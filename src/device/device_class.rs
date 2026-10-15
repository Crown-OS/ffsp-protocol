use crate::device::DeviceError;

/// What kind of device a peer is, so a UI can pick an icon and the scheduler a power policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceClass {
    Laptop = 1,
    Desktop = 2,
    Phone = 3,
    Tablet = 4,
}

impl DeviceClass {
    pub const fn from_byte(byte: u8) -> Result<Self, DeviceError> {
        match byte {
            1 => Ok(Self::Laptop),
            2 => Ok(Self::Desktop),
            3 => Ok(Self::Phone),
            4 => Ok(Self::Tablet),
            other => Err(DeviceError::UnknownDeviceClass(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    /// Mains-powered devices tolerate the scheduler keeping every lane warm.
    pub const fn is_usually_mains_powered(self) -> bool {
        matches!(self, Self::Laptop | Self::Desktop)
    }
}
