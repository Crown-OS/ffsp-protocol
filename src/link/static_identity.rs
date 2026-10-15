use core::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::device::DeviceId;

/// A device's long-term X25519 keypair.
///
/// The secret has to be in process memory to run a handshake — the Android Keystore does not do
/// X25519 agreement — so it is held in a wrapper that wipes on drop and never prints itself. It is
/// stored at rest encrypted by the platform's key store.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct StaticIdentity {
    secret: [u8; Self::KEY_LEN],
    public: [u8; Self::KEY_LEN],
}

impl StaticIdentity {
    pub const KEY_LEN: usize = 32;

    pub const fn new(secret: [u8; Self::KEY_LEN], public: [u8; Self::KEY_LEN]) -> Self {
        Self { secret, public }
    }

    pub const fn secret_key(&self) -> &[u8; Self::KEY_LEN] {
        &self.secret
    }

    pub const fn public_key(&self) -> &[u8; Self::KEY_LEN] {
        &self.public
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::from_public_key(&self.public)
    }
}

impl fmt::Debug for StaticIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "StaticIdentity({})", self.device_id())
    }
}
