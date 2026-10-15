use crate::control::ControlError;

/// Why a transfer will not happen, or will not continue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EndReason {
    UserRejected = 1,
    NotEnoughSpace = 2,
    Unsupported = 3,
    Busy = 4,
    UserCancelled = 5,
    Failed = 6,
}

impl EndReason {
    pub const fn from_byte(byte: u8) -> Result<Self, ControlError> {
        match byte {
            1 => Ok(Self::UserRejected),
            2 => Ok(Self::NotEnoughSpace),
            3 => Ok(Self::Unsupported),
            4 => Ok(Self::Busy),
            5 => Ok(Self::UserCancelled),
            6 => Ok(Self::Failed),
            other => Err(ControlError::UnknownReason(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}
