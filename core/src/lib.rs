//! Inkling's shared core. No UI framework may be a dependency of this crate.
//!
//! Offsets are byte offsets into the document's UTF-8 text, always on a char boundary.
//! A UI that counts in UTF-16 converts at its own edge.

pub mod ai;
pub mod ask;
pub mod feedback;
#[cfg(test)]
mod mock;
pub mod providers;
pub mod review;
pub mod store;

pub use ai::{AiEvent, Backend, Cancel, Prompt, Provider, start};
pub use ask::{AnswerError, Ask, BadRequest, Request, Revision, parse_alternatives};
pub use feedback::{Build, BuildNumber, Desk};
pub use review::{Change, ChangeId, Decision, Edit, Review, Segment, Stats};
pub use store::{KeyStore, MemoryKeys, Paths, Secret, Settings, backend_for_request, default_keys, headless};
#[cfg(target_vendor = "apple")]
pub use store::KeychainKeys;
