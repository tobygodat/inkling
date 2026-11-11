use crate::ai::{Backend, Provider};
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
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub provider: Provider,
    #[serde(default)]
    models: BTreeMap<Provider, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { provider: Provider::Claude, models: BTreeMap::new() }
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
        self.models.get(&provider).map(String::as_str).filter(|m| !m.trim().is_empty()).unwrap_or(provider.default_model())
    }

    pub fn set_model(&mut self, provider: Provider, model: &str) {
        self.models.insert(provider, model.trim().to_string());
    }
}

/// API keys never go in the settings file.
pub trait KeyStore: Send + Sync {
    fn get(&self, provider: Provider) -> Option<String>;
    fn set(&self, provider: Provider, key: &str) -> Result<(), String>;
    fn remove(&self, provider: Provider) -> Result<(), String>;
}

#[derive(Default)]
pub struct MemoryKeys(Mutex<HashMap<Provider, String>>);

impl KeyStore for MemoryKeys {
    fn get(&self, provider: Provider) -> Option<String> {
        self.0.lock().unwrap().get(&provider).cloned()
    }

    fn set(&self, provider: Provider, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(provider, key.to_string());
        Ok(())
    }

    fn remove(&self, provider: Provider) -> Result<(), String> {
        self.0.lock().unwrap().remove(&provider);
        Ok(())
    }
}

/// API keys in the Apple Keychain: generic passwords under service `com.tobygodat.inkling`,
/// one account per `Provider::id()`.
#[cfg(target_vendor = "apple")]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeychainKeys;

#[cfg(target_vendor = "apple")]
impl KeychainKeys {
    pub const SERVICE: &'static str = "com.tobygodat.inkling";
}

#[cfg(target_vendor = "apple")]
impl KeyStore for KeychainKeys {
    fn get(&self, provider: Provider) -> Option<String> {
        let bytes = security_framework::passwords::get_generic_password(Self::SERVICE, provider.id()).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn set(&self, provider: Provider, key: &str) -> Result<(), String> {
        security_framework::passwords::set_generic_password(Self::SERVICE, provider.id(), key.trim().as_bytes())
            .map_err(|e| format!("Couldn't save the {} key to the Keychain ({e}).", provider.label()))
    }

    fn remove(&self, provider: Provider) -> Result<(), String> {
        const NOT_FOUND: i32 = -25300; // errSecItemNotFound
        match security_framework::passwords::delete_generic_password(Self::SERVICE, provider.id()) {
            Err(e) if e.code() != NOT_FOUND => {
                Err(format!("Couldn't remove the {} key from the Keychain ({e}).", provider.label()))
            }
            _ => Ok(()),
        }
    }
}

/// The key store an app should use: the Keychain on Apple platforms. `INKLING_FAKE_AI` or
/// `INKLING_MEMORY_KEYS` keeps keys in memory instead, so headless runs never touch the Keychain.
pub fn default_keys() -> Box<dyn KeyStore> {
    let in_memory = ["INKLING_FAKE_AI", "INKLING_MEMORY_KEYS"].iter().any(|v| std::env::var_os(v).is_some());
    if in_memory || cfg!(not(target_vendor = "apple")) {
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

/// The backend a request should use. `INKLING_FAKE_AI=1` forces the fake one, for headless runs.
/// `Err` names the provider whose key is missing, so the UI can open settings.
pub fn backend_for(settings: &Settings, keys: &dyn KeyStore) -> Result<Backend, Provider> {
    if std::env::var_os("INKLING_FAKE_AI").is_some() {
        return Ok(Backend::Fake { text: FAKE_TEXT.into(), word_delay: Duration::from_millis(40) });
    }
    let provider = settings.provider;
    let key = keys.get(provider).filter(|k| !k.trim().is_empty()).ok_or(provider)?;
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
        if std::env::var_os("INKLING_FAKE_AI").is_none() {
            assert_eq!(backend_for(&settings, &keys), Err(Provider::Claude));
            keys.set(Provider::Claude, "sk-test").unwrap();
            assert!(matches!(backend_for(&settings, &keys), Ok(Backend::Live { .. })));
        }
    }
}
