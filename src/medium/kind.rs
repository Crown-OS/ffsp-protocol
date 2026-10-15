use core::fmt;

use crate::medium::MediumError;

/// A way two devices can reach each other.
///
/// The byte values are wire-visible and mirrored by `Medium.kt` on Android; a test asserts the two
/// stay in step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Medium {
    Loopback = 1,
    WifiInfra = 2,
    WifiDirect = 3,
    UsbAccessory = 4,
    BluetoothLe = 5,
    BluetoothClassic = 6,
}

impl Medium {
    pub const ALL: [Self; 6] = [
        Self::Loopback,
        Self::WifiInfra,
        Self::WifiDirect,
        Self::UsbAccessory,
        Self::BluetoothLe,
        Self::BluetoothClassic,
    ];

    pub const fn from_byte(byte: u8) -> Result<Self, MediumError> {
        match byte {
            1 => Ok(Self::Loopback),
            2 => Ok(Self::WifiInfra),
            3 => Ok(Self::WifiDirect),
            4 => Ok(Self::UsbAccessory),
            5 => Ok(Self::BluetoothLe),
            6 => Ok(Self::BluetoothClassic),
            other => Err(MediumError::UnknownMedium(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}

impl fmt::Display for Medium {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Loopback => "loopback",
            Self::WifiInfra => "Wi-Fi",
            Self::WifiDirect => "Wi-Fi Direct",
            Self::UsbAccessory => "USB",
            Self::BluetoothLe => "Bluetooth LE",
            Self::BluetoothClassic => "Bluetooth",
        };
        formatter.write_str(name)
    }
}
