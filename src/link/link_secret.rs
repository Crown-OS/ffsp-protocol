use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Shared secret established when two devices are linked, and never sent again.
///
/// Discovery beacons carry a rotating keyed hash of it rather than the secret or any stable
/// identifier, so only a device that already holds it can tell who is advertising.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct LinkSecret([u8; Self::LEN]);

impl LinkSecret {
    pub const LEN: usize = 32;

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

/// Withholds the key material, so a debug log of a [`LinkRecord`](crate::link::LinkRecord) is safe.
impl fmt::Debug for LinkSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("LinkSecret(redacted)")
    }
}
