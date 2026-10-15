//! What this crate needs from the operating system, and nothing more.

mod clock;
mod entropy;
mod power;

pub use clock::{Clock, SimulatedClock, SystemClock};
pub use entropy::Entropy;
pub use power::PowerHint;
