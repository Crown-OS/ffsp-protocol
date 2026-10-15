//! One linked peer, its lanes, and the keys protecting them.

mod error;
mod peer_session;
mod state;

pub use error::SessionError;
pub use peer_session::{LaneKeys, Session};
pub use state::{SessionSignal, SessionState};

#[cfg(test)]
mod tests;
