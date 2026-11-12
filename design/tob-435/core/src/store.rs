//! SKETCH: changes to core/src/store.rs. Only the parts that change are shown.
//!
//! The feedback link is stored through the existing `KeyStore`, in the Keychain, never a file
//! (decision 4). `KeyStore` is keyed today by `Provider`; it becomes keyed by `Secret`, the one
//! list of everything the Keychain holds. `Secret::account()` is the Keychain account, so the
//! three existing keys keep their accounts ("claude", "gemini", "chatgpt") and nothing migrates.

use crate::ai::Provider;
use std::path::PathBuf;

/// Everything Inkling keeps in the key store. Adding a secret is adding a variant here; the
/// compiler then points at `account` and `label`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Secret {
    ApiKey(Provider),
    FeedbackLink,
}

impl Secret {
    /// The Keychain account under service `com.tobygodat.inkling`.
    pub fn account(self) -> &'static str {
        match self {
            Secret::ApiKey(p) => p.id(),
            Secret::FeedbackLink => "feedback-link",
        }
    }

    /// For error messages: "Couldn't save the {label} to the Keychain".
    pub fn label(self) -> String {
        match self {
            Secret::ApiKey(p) => format!("{} key", p.label()),
            Secret::FeedbackLink => "feedback link".into(),
        }
    }
}

/// Unchanged contract, new key type. Callers write `keys.get(Secret::ApiKey(provider))`.
pub trait KeyStore: Send + Sync {
    fn get(&self, secret: Secret) -> Option<String>;
    fn set(&self, secret: Secret, value: &str) -> Result<(), String>;
    fn remove(&self, secret: Secret) -> Result<(), String>;
    fn has(&self, secret: Secret) -> bool {
        self.get(secret).is_some_and(|k| !k.trim().is_empty())
    }
}

// MemoryKeys: HashMap<Secret, String>.
// KeychainKeys: `provider.id()` → `secret.account()`, `provider.label()` → `secret.label()`.

/// A store shared across relaunches inside one process, for the Mac snap: it relaunches the app
/// "as the next build" and the link must still be saved, as it would be in the Keychain.
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

pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    /// What this device has sent. See `feedback::load_sent`.
    pub fn sent_feedback(&self) -> PathBuf {
        self.dir.join("sent-feedback.json")
    }
}

// `write_atomic` becomes `pub(crate)` so `feedback` writes its log the same way.
