//! SKETCH: changes to ui/src/lib.rs.

mod feedback; // new

pub use app::{Inkling, OpenKeys, Saved};
pub use feedback::Restart; // the one event the host subscribes to

impl FormFactor {
    /// For the feedback body's `form_factor`: "mac" or "phone".
    pub fn id(self) -> &'static str {
        match self {
            FormFactor::Mac => "mac",
            FormFactor::Phone => "phone",
        }
    }
}
