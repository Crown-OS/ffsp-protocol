use core::fmt;

use crate::discovery::Epoch;
use crate::link::LinkSecret;

/// The rotating value a device advertises instead of a stable identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EphemeralId([u8; Self::LEN]);

impl EphemeralId {
    pub const LEN: usize = 16;

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for EphemeralId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EphemeralId(")?;
        for byte in self.0.iter().take(4) {
            write!(formatter, "{byte:02x}")?;
        }
        formatter.write_str("..)")
    }
}

/// Derives the identifier a linked device advertises during `epoch`.
///
/// Keyed BLAKE3 rather than HMAC-SHA256: it is a MAC with the same guarantee, it is already a
/// dependency here, and it is faster on both targets.
pub fn ephemeral_id(secret: &LinkSecret, epoch: Epoch) -> EphemeralId {
    let mac = blake3::keyed_hash(secret.as_bytes(), &epoch.as_u64().to_le_bytes());
    let [
        truncated @ ..,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
        _,
    ] = *mac.as_bytes();
    EphemeralId(truncated)
}
