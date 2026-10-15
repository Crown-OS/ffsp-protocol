use crate::control::ControlError;

/// What a [`FrameKind::Control`](crate::wire::FrameKind) body says.
///
/// The byte is first in the body so a receiver can dispatch before it has parsed anything else,
/// and so an unknown message from a newer peer names itself in the error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlOpcode {
    TransferOffer = 1,
    TransferAccept = 2,
    TransferDecline = 3,
    TransferCancel = 4,
    ChunkMap = 5,
    LaneInvite = 6,
    BenchmarkMark = 7,
    TransferReady = 8,
}

impl ControlOpcode {
    pub const fn from_byte(byte: u8) -> Result<Self, ControlError> {
        match byte {
            1 => Ok(Self::TransferOffer),
            2 => Ok(Self::TransferAccept),
            3 => Ok(Self::TransferDecline),
            4 => Ok(Self::TransferCancel),
            5 => Ok(Self::ChunkMap),
            6 => Ok(Self::LaneInvite),
            7 => Ok(Self::BenchmarkMark),
            8 => Ok(Self::TransferReady),
            other => Err(ControlError::UnknownOpcode(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }
}
