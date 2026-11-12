//! SKETCH: changes to core/src/lib.rs.

pub mod feedback; // new: Desk, Build, the link, the records, the one POST
pub mod store;    // KeyStore now keyed by Secret

pub use feedback::{Build, BuildNumber, Desk};
pub use store::Secret;
// `KeyStore`, `Paths` and the rest stay as they are. Cargo.toml: `ring = "0.17"` (already in the
// tree through rustls; gives HMAC-SHA256 and the OS random source), and [build-dependencies]
// serde + serde_json.
