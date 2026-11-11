//! Inkling's shared core. No UI framework may be a dependency of this crate.
//!
//! Offsets are byte offsets into the document's UTF-8 text, always on a char boundary.
//! A UI that counts in UTF-16 converts at its own edge.

pub mod ai;
pub mod assist;
pub mod providers;
pub mod store;

pub use ai::{Action, AiEvent, AiRequest, Backend, Cancel, Prompt, Provider, start};
pub use assist::{Assist, Proposal};
pub use store::{KeyStore, MemoryKeys, Paths, Settings, backend_for, default_keys};
