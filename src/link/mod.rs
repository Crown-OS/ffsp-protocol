//! Persistent trust between two devices.

mod error;
mod link_record;
mod link_secret;
mod link_store;
mod pairing;
mod static_identity;
mod verification;

pub use error::PairingError;
pub use link_record::LinkRecord;
pub use link_secret::LinkSecret;
pub use link_store::{LinkStore, LinkStoreError};
pub use pairing::{Handshake, HandshakeOutcome, generate_identity};
pub use static_identity::StaticIdentity;
pub use verification::VerificationCode;

#[cfg(test)]
mod tests;
