use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto::CipherSuite;

/// One lane's AEAD key, in one direction.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct LaneKey([u8; CipherSuite::KEY_LEN]);

impl LaneKey {
    pub const fn from_bytes(bytes: [u8; CipherSuite::KEY_LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; CipherSuite::KEY_LEN] {
        &self.0
    }
}

impl fmt::Debug for LaneKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LaneKey(redacted)")
    }
}
