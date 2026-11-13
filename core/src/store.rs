use crate::ai::{Backend, Provider};
use crate::appearance::{Appearance, ReadingLook, View, lenient};
use crate::ask::Request;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use std::{fs, io};

/// Where one install keeps its files. The Mac uses `Paths::mac()`; an iOS host passes its own folder.
#[derive(Clone, Debug)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    /// `INKLING_DATA_DIR` when set, else `~/Library/Application Support/Inkling`.
    pub fn mac() -> Self {
        let dir = std::env::var_os("INKLING_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Library/Application Support/Inkling")
        });
        Paths { dir }
    }

    pub fn draft(&self) -> PathBuf {
        self.dir.join("draft.md")
    }

    pub fn settings(&self) -> PathBuf {
        self.dir.join("settings.json")
    }

    pub fn load_draft(&self) -> String {
        fs::read_to_string(self.draft()).unwrap_or_default()
    }

    pub fn save_draft(&self, text: &str) -> io::Result<()> {
        write_atomic(&self.draft(), text.as_bytes())
    }

    /// What this device has sent as feedback. See `feedback::Desk`.
    pub fn sent_feedback(&self) -> PathBuf {
        self.dir.join("sent-feedback.json")
    }
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Stored")]
pub struct Settings {
    pub provider: Provider,
    models: BTreeMap<Provider, String>,
    /// The writing look. Missing in a file written before the Appearance panel, and then the
    /// default look.
    pub appearance: Appearance,
    /// Reading's look and layout. Change a look through `change_look` and `set_synced`, which keep
    /// the two in step while they are synced.
    pub reading: ReadingLook,
}

/// `settings.json` as read, before the two looks are put in step.
#[derive(Deserialize)]
struct Stored {
    provider: Provider,
    #[serde(default)]
    models: BTreeMap<Provider, String>,
    #[serde(default, deserialize_with = "lenient")]
    appearance: Appearance,
    /// Missing in a file written before reading had its own look, and junk reads as missing:
    /// then reading takes the writing look, synced.
    #[serde(default, deserialize_with = "lenient")]
    reading: Option<ReadingLook>,
}

impl From<Stored> for Settings {
    fn from(s: Stored) -> Self {
        let mut reading = s.reading.unwrap_or_default();
        if reading.synced {
            reading.look.take_look(&s.appearance);
        }
        Settings { provider: s.provider, models: s.models, appearance: s.appearance, reading }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings { provider: Provider::Claude, models: BTreeMap::new(), appearance: Appearance::default(), reading: ReadingLook::default() }
    }
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        fs::read(paths.settings()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> io::Result<()> {
        write_atomic(&paths.settings(), &serde_json::to_vec_pretty(self)?)
    }

    pub fn model(&self, provider: Provider) -> &str {
        self.typed_model(provider).unwrap_or(provider.default_model())
    }

    /// The model the writer typed for `provider`, or `None` when they left the default.
    pub fn typed_model(&self, provider: Provider) -> Option<&str> {
        self.models.get(&provider).map(String::as_str).filter(|m| !m.trim().is_empty())
    }

    /// The look `view` draws with.
    pub fn look(&self, view: View) -> &Appearance {
        match view {
            View::Writing => &self.appearance,
            View::Reading => &self.reading.look,
        }
    }

    /// Changes `view`'s look. While the two are synced the other takes the same look; its own
    /// page switches stay as they are.
    pub fn change_look(&mut self, view: View, change: impl FnOnce(&mut Appearance)) {
        let (changed, other) = match view {
            View::Writing => (&mut self.appearance, &mut self.reading.look),
            View::Reading => (&mut self.reading.look, &mut self.appearance),
        };
        change(changed);
        if self.reading.synced {
            other.take_look(changed);
        }
    }

    /// Turns sync on or off. Turning it on gives the other view `from`'s look.
    pub fn set_synced(&mut self, on: bool, from: View) {
        self.reading.synced = on;
        if on {
            self.change_look(from, |_| ());
        }
    }

    pub fn set_model(&mut self, provider: Provider, model: &str) {
        self.models.insert(provider, model.trim().to_string());
    }
}

/// Everything Inkling keeps in the key store. Adding a secret is adding a variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Secret {
    ApiKey(Provider),
    FeedbackLink,
}

impl Secret {
    /// The Keychain account under service `com.tobygodat.inkling`. API keys keep the accounts
    /// they had before the feedback link existed.
    pub fn account(self) -> &'static str {
        match self {
            Secret::ApiKey(p) => p.id(),
            Secret::FeedbackLink => "feedback-link",
        }
    }

    /// How the Keychain's errors name it. Only the Keychain, and so only Apple builds, use it.
    #[cfg(target_vendor = "apple")]
    fn label(self) -> String {
        match self {
            Secret::ApiKey(p) => format!("{} key", p.label()),
            Secret::FeedbackLink => "feedback link".into(),
        }
    }
}

/// API keys and the feedback link. Never the settings file.
pub trait KeyStore: Send + Sync {
    fn get(&self, secret: Secret) -> Option<String>;
    fn set(&self, secret: Secret, value: &str) -> Result<(), String>;
    fn remove(&self, secret: Secret) -> Result<(), String>;

    /// Whether `secret` is saved. Stores that can answer without reading the secret do, so a
    /// settings screen can show "saved" without unlocking anything.
    fn has(&self, secret: Secret) -> bool {
        self.get(secret).is_some_and(|k| !k.trim().is_empty())
    }
}

/// One store shared by several views, so a headless run can relaunch the app and find what it
/// saved, as it would in the Keychain.
impl<T: KeyStore + ?Sized> KeyStore for std::sync::Arc<T> {
    fn get(&self, secret: Secret) -> Option<String> {
        (**self).get(secret)
    }

    fn set(&self, secret: Secret, value: &str) -> Result<(), String> {
        (**self).set(secret, value)
    }

    fn remove(&self, secret: Secret) -> Result<(), String> {
        (**self).remove(secret)
    }

    fn has(&self, secret: Secret) -> bool {
        (**self).has(secret)
    }
}

#[derive(Default)]
pub struct MemoryKeys(Mutex<HashMap<Secret, String>>);

impl KeyStore for MemoryKeys {
    fn get(&self, secret: Secret) -> Option<String> {
        self.0.lock().unwrap().get(&secret).cloned()
    }

    fn set(&self, secret: Secret, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(secret, value.to_string());
        Ok(())
    }

    fn remove(&self, secret: Secret) -> Result<(), String> {
        self.0.lock().unwrap().remove(&secret);
        Ok(())
    }
}

/// Secrets in the Apple Keychain: generic passwords under service `com.tobygodat.inkling`,
/// one account per `Secret::account()`.
#[cfg(target_vendor = "apple")]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeychainKeys;

#[cfg(target_vendor = "apple")]
impl KeychainKeys {
    pub const SERVICE: &'static str = "com.tobygodat.inkling";
}

#[cfg(target_vendor = "apple")]
impl KeyStore for KeychainKeys {
    fn get(&self, secret: Secret) -> Option<String> {
        let bytes = security_framework::passwords::get_generic_password(Self::SERVICE, secret.account()).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn set(&self, secret: Secret, value: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(Self::SERVICE, secret.account(), value.trim().as_bytes())
            .map_err(|e| format!("Couldn't save the {} to the Keychain ({e}).", secret.label()))
    }

    /// Asks the Keychain whether the item exists without loading its data or attributes.
    fn has(&self, secret: Secret) -> bool {
        use security_framework::item::{ItemClass, ItemSearchOptions};
        ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .service(Self::SERVICE)
            .account(secret.account())
            .search()
            .is_ok()
    }

    fn remove(&self, secret: Secret) -> Result<(), String> {
        const NOT_FOUND: i32 = -25300; // errSecItemNotFound
        match security_framework::passwords::delete_generic_password(Self::SERVICE, secret.account()) {
            Err(e) if e.code() != NOT_FOUND => {
                Err(format!("Couldn't remove the {} from the Keychain ({e}).", secret.label()))
            }
            _ => Ok(()),
        }
    }
}

/// Whether this is a headless run: `INKLING_FAKE_AI` or `INKLING_MEMORY_KEYS`. A headless run
/// keeps keys in memory and never sends feedback over the network.
pub fn headless() -> bool {
    ["INKLING_FAKE_AI", "INKLING_MEMORY_KEYS"].iter().any(|v| std::env::var_os(v).is_some())
}

/// The key store an app should use: the Keychain on Apple platforms, memory in a `headless()` run.
pub fn default_keys() -> Box<dyn KeyStore> {
    if headless() || cfg!(not(target_vendor = "apple")) {
        return Box::new(MemoryKeys::default());
    }
    keychain()
}

#[cfg(target_vendor = "apple")]
fn keychain() -> Box<dyn KeyStore> {
    Box::new(KeychainKeys)
}

#[cfg(not(target_vendor = "apple"))]
fn keychain() -> Box<dyn KeyStore> {
    Box::new(MemoryKeys::default())
}

pub const FAKE_TEXT: &str = "The kettle clicked off in the next room, and for a moment the house held its breath with her.";

/// The backend a request should use. `Err` names the provider whose key is missing, so the UI
/// can open the key screen. `INKLING_FAKE_AI=1` forces the fake one, for headless runs: it streams
/// a canned answer made from the request's draft, so a headless run gets a real review to show.
pub fn backend_for_request(settings: &Settings, keys: &dyn KeyStore, request: &Request) -> Result<Backend, Provider> {
    if std::env::var_os("INKLING_FAKE_AI").is_some() {
        return Ok(Backend::Fake { text: request.fake_answer(), word_delay: Duration::from_millis(40) });
    }
    let provider = settings.provider;
    let key = keys.get(Secret::ApiKey(provider)).filter(|k| !k.trim().is_empty()).ok_or(provider)?;
    Ok(Backend::Live { provider, model: settings.model(provider).to_string(), key })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> Paths {
        let dir = std::env::temp_dir().join(format!("inkling-test-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = fs::remove_dir_all(&dir);
        Paths { dir }
    }

    #[test]
    fn draft_and_settings_survive_a_reload() {
        let paths = temp();
        assert_eq!(paths.load_draft(), "");
        paths.save_draft("héllo\n").unwrap();
        let mut s = Settings::load(&paths);
        s.provider = Provider::Gemini;
        s.set_model(Provider::Gemini, " custom-model ");
        s.save(&paths).unwrap();
        assert_eq!(paths.load_draft(), "héllo\n");
        let back = Settings::load(&paths);
        assert_eq!(back, s);
        assert_eq!(back.model(Provider::Gemini), "custom-model");
        assert_eq!(back.model(Provider::Claude), Provider::Claude.default_model());
    }

    #[test]
    fn a_missing_key_names_its_provider() {
        let keys = MemoryKeys::default();
        let settings = Settings::default();
        let request = Request::new("Once", crate::ask::Ask::FixGrammar).unwrap();
        if std::env::var_os("INKLING_FAKE_AI").is_none() {
            assert_eq!(backend_for_request(&settings, &keys, &request), Err(Provider::Claude));
            keys.set(Secret::ApiKey(Provider::Claude), "sk-test").unwrap();
            assert!(matches!(backend_for_request(&settings, &keys, &request), Ok(Backend::Live { .. })));
        }
    }
}
