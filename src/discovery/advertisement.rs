use crate::device::{Capabilities, DeviceClass};
use crate::discovery::{DiscoveryError, EphemeralId, Epoch};
use crate::wire::ProtocolVersion;

/// Bytes of service data a legacy BLE advertisement can carry.
///
/// A legacy advertising PDU is 31 bytes: three for the flags structure and four for a service-data
/// header carrying a **16-bit** UUID, leaving exactly 24. A 128-bit UUID would consume 18 of those
/// 31 and leave six, which is why the service UUID below is 16-bit.
pub const ADVERTISEMENT_LEN: usize = 24;

/// 16-bit service UUID the beacon is published under.
///
/// A collision with another 16-bit assignment is harmless: the version byte and the ephemeral
/// identifier both have to resolve before a beacon is treated as a peer.
pub const SERVICE_UUID_16: u16 = 0xFCF1;

/// What a device broadcasts so linked peers can recognise it.
///
/// Deliberately absent: the device name, which is readable by anyone in radio range. A name is
/// only exchanged once a peer has proved it holds the link secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Advertisement {
    pub version: ProtocolVersion,
    pub device_class: DeviceClass,
    pub ephemeral_id: EphemeralId,
    pub epoch_low: u8,
    pub capabilities: Capabilities,
}

impl Advertisement {
    pub const fn new(
        device_class: DeviceClass,
        ephemeral_id: EphemeralId,
        epoch: Epoch,
        capabilities: Capabilities,
    ) -> Self {
        Self {
            version: ProtocolVersion::CURRENT,
            device_class,
            ephemeral_id,
            epoch_low: epoch.low_byte(),
            capabilities,
        }
    }

    /// Returns a fixed-size array, so overrunning the 24-byte budget cannot compile.
    ///
    /// BlueZ truncates over-long service data silently, which would otherwise produce a beacon
    /// that looks correct and resolves to nothing.
    pub const fn to_bytes(&self) -> [u8; ADVERTISEMENT_LEN] {
        let mut out = [0u8; ADVERTISEMENT_LEN];
        {
            let [
                version,
                class,
                id @ ..,
                epoch_low,
                cap_low,
                cap_high,
                _,
                _,
                _,
            ] = &mut out;
            *version = self.version.as_byte();
            *class = self.device_class.as_byte();
            *id = *self.ephemeral_id.as_bytes();
            *epoch_low = self.epoch_low;
            let [low, high] = self.capabilities.bits().to_le_bytes();
            *cap_low = low;
            *cap_high = high;
        }
        out
    }

    pub fn parse(source: &[u8]) -> Result<Self, DiscoveryError> {
        let source: &[u8; ADVERTISEMENT_LEN] =
            source
                .try_into()
                .map_err(|_| DiscoveryError::AdvertisementLength {
                    expected: ADVERTISEMENT_LEN,
                    actual: source.len(),
                })?;
        let &[
            version,
            class,
            ref id @ ..,
            epoch_low,
            cap_low,
            cap_high,
            _,
            _,
            _,
        ] = source;

        let version = ProtocolVersion::new(version);
        if !version.is_supported() {
            return Err(DiscoveryError::UnsupportedVersion(version.as_byte()));
        }
        Ok(Self {
            version,
            device_class: DeviceClass::from_byte(class)?,
            ephemeral_id: EphemeralId::from_bytes(*id),
            epoch_low,
            capabilities: Capabilities::from_bits_lossy(u16::from_le_bytes([cap_low, cap_high])),
        })
    }
}
