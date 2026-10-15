use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::crypto::{CipherSuite, LaneKey};
use crate::wire::LaneId;

/// Which side of the session a key belongs to.
///
/// Each direction gets its own key, so the two peers never share a nonce counter and neither has
/// to coordinate with the other before encrypting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Initiator,
    Responder,
}

impl Direction {
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::Initiator => 1,
            Self::Responder => 2,
        }
    }

    pub const fn peer(self) -> Self {
        match self {
            Self::Initiator => Self::Responder,
            Self::Responder => Self::Initiator,
        }
    }
}

const LANE_KEY_CONTEXT: &str = "crownos.ffsp.v1 lane key";
const RESUME_KEY_CONTEXT: &str = "crownos.ffsp.v1 resume state key";

/// The secret a completed handshake leaves behind.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct SessionSecret([u8; CipherSuite::KEY_LEN]);

impl SessionSecret {
    pub const fn from_bytes(bytes: [u8; CipherSuite::KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// A distinct key per lane and direction.
    ///
    /// Deriving from the lane identifier is what lets every lane encrypt in parallel with no
    /// shared counter, and it is why [`LaneRegistry`](crate::medium::LaneRegistry) must never
    /// reissue an identifier: a repeated lane would restart a nonce sequence under a key that had
    /// already used it.
    pub fn lane_key(&self, direction: Direction, lane: LaneId) -> LaneKey {
        let mut hasher = blake3::Hasher::new_derive_key(LANE_KEY_CONTEXT);
        hasher.update(&self.0);
        hasher.update(&[direction.as_byte()]);
        hasher.update(&lane.as_u16().to_le_bytes());
        LaneKey::from_bytes(*hasher.finalize().as_bytes())
    }

    /// Key for encrypting persisted resume state at rest.
    pub fn resume_key(&self) -> LaneKey {
        let mut hasher = blake3::Hasher::new_derive_key(RESUME_KEY_CONTEXT);
        hasher.update(&self.0);
        LaneKey::from_bytes(*hasher.finalize().as_bytes())
    }
}

impl core::fmt::Debug for SessionSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("SessionSecret(redacted)")
    }
}
