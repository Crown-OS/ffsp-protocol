use core::fmt;

/// Stable identity of a device, derived from its long-term public key.
///
/// Deriving rather than using the key directly keeps the key out of discovery beacons and logs
/// while still being unforgeable: only the holder of the secret can prove ownership of the id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId([u8; Self::LEN]);

impl DeviceId {
    pub const LEN: usize = 32;
    const CONTEXT: &'static str = "crownos.ffsp.v1.device-id";

    pub fn from_public_key(public_key: &[u8; 32]) -> Self {
        let mut hasher = blake3::Hasher::new_derive_key(Self::CONTEXT);
        hasher.update(public_key);
        Self(*hasher.finalize().as_bytes())
    }

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "DeviceId({self})")
    }
}

/// Renders the leading four bytes, which is what a pairing screen shows.
impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0.iter().take(4) {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
