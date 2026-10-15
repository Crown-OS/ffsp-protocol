use super::WireError;

/// Zero is deliberately unassigned so a run of zero bytes never parses as a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    Handshake = 1,
    Control = 2,
    Chunk = 3,
    ChunkAck = 4,
    LaneReport = 5,
    KeepAlive = 6,
}

impl FrameKind {
    pub const fn from_byte(byte: u8) -> Result<Self, WireError> {
        match byte {
            1 => Ok(Self::Handshake),
            2 => Ok(Self::Control),
            3 => Ok(Self::Chunk),
            4 => Ok(Self::ChunkAck),
            5 => Ok(Self::LaneReport),
            6 => Ok(Self::KeepAlive),
            other => Err(WireError::UnknownFrameKind(other)),
        }
    }

    pub const fn as_byte(self) -> u8 {
        self as u8
    }

    pub const fn carries_bulk_payload(self) -> bool {
        matches!(self, Self::Chunk)
    }
}
