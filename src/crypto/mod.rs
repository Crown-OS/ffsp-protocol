//! Authenticated encryption, keyed per lane so lanes never coordinate.

mod aead;
mod cipher_suite;
mod cpu_features;
mod error;
mod kdf;
mod lane_key;
mod nonce;

pub use aead::LaneCipher;
pub use cipher_suite::CipherSuite;
pub use cpu_features::has_aes_hardware;
pub use error::CryptoError;
pub use kdf::{Direction, SessionSecret};
pub use lane_key::LaneKey;
pub use nonce::NonceSequence;

#[cfg(test)]
mod tests;
