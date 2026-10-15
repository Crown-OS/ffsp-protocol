use super::{FrameKind, LaneId, MAX_BODY_LEN, SequenceNumber, TAG_LEN, WireError};
use crate::util::bytes;

/// The eight bytes in front of every frame.
///
/// These same bytes serve three roles at once: the length prefix a stream lane needs to find the
/// frame boundary, the routing header, and the associated data the AEAD authenticates. Encoding
/// once and reusing the array for all three is what keeps per-chunk overhead at 28 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    body_len: u32,
    kind: FrameKind,
    lane: LaneId,
    seq: SequenceNumber,
}

impl FrameHeader {
    pub const LEN: usize = 8;

    pub fn new(
        kind: FrameKind,
        lane: LaneId,
        seq: SequenceNumber,
        body_len: usize,
    ) -> Result<Self, WireError> {
        if body_len > MAX_BODY_LEN {
            return Err(WireError::BodyTooLong(body_len));
        }
        let body_len = u32::try_from(body_len).map_err(|_| WireError::BodyTooLong(body_len))?;
        Ok(Self {
            body_len,
            kind,
            lane,
            seq,
        })
    }

    pub fn parse(source: &[u8]) -> Result<Self, WireError> {
        let truncated = || WireError::Truncated {
            needed: Self::LEN,
            available: source.len(),
        };
        let body_len = bytes::read_u24_le(source, 0).ok_or_else(truncated)?;
        let kind = FrameKind::from_byte(*source.get(3).ok_or_else(truncated)?)?;
        let lane = LaneId::new(bytes::read_u16_le(source, 4).ok_or_else(truncated)?);
        let seq = SequenceNumber::new(bytes::read_u16_le(source, 6).ok_or_else(truncated)?);
        Ok(Self {
            body_len,
            kind,
            lane,
            seq,
        })
    }

    pub const fn to_bytes(self) -> [u8; Self::LEN] {
        let [len_low, len_mid, len_high] = bytes::u24_le(self.body_len);
        let [lane_low, lane_high] = self.lane.as_u16().to_le_bytes();
        let [seq_low, seq_high] = self.seq.as_u16().to_le_bytes();
        [
            len_low,
            len_mid,
            len_high,
            self.kind.as_byte(),
            lane_low,
            lane_high,
            seq_low,
            seq_high,
        ]
    }

    pub const fn body_len(self) -> usize {
        self.body_len as usize
    }

    pub const fn kind(self) -> FrameKind {
        self.kind
    }

    pub const fn lane(self) -> LaneId {
        self.lane
    }

    pub const fn seq(self) -> SequenceNumber {
        self.seq
    }

    /// Total bytes this frame occupies on a lane, including the header and the AEAD tag.
    pub const fn sealed_frame_len(self) -> usize {
        Self::LEN + self.body_len as usize + TAG_LEN
    }
}
