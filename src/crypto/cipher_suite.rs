use crate::crypto::{CryptoError, has_aes_hardware};
use crate::device::Capabilities;

/// Both suites are AEADs with a 256-bit key, a 96-bit nonce and a 128-bit tag, so the framing is
/// identical either way and only the primitive changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CipherSuite {
    ChaCha20Poly1305 = 1,
    Aes256Gcm = 2,
}

impl CipherSuite {
    pub const KEY_LEN: usize = 32;
    pub const NONCE_LEN: usize = 12;
    pub const TAG_LEN: usize = 16;

    /// AES only when *both* peers accelerate it.
    ///
    /// Software AES is several times slower than ChaCha20, so a phone without ARMv8 crypto
    /// extensions would be the bottleneck for a laptop that has AES-NI. Choosing on the
    /// intersection means the slower peer decides, which is the peer that matters.
    pub const fn negotiate(local: Capabilities, peer: Capabilities) -> Self {
        if local.both_accelerate_aes(peer) {
            Self::Aes256Gcm
        } else {
            Self::ChaCha20Poly1305
        }
    }

    /// The capability bit this processor should advertise.
    pub fn local_capability() -> Capabilities {
        if has_aes_hardware() {
            Capabilities::AES_HARDWARE
        } else {
            Capabilities::empty()
        }
    }

    pub const fn from_byte(byte: u8) -> Result<Self, CryptoError> {
        match byte {
            1 => Ok(Self::ChaCha20Poly1305),
            2 => Ok(Self::Aes256Gcm),
            other => Err(CryptoError::UnknownCipherSuite(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}
