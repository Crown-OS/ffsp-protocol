use super::{CHUNK_HEADER_LEN, ChunkIndex, WireError};
use crate::util::bytes;

/// A decrypted chunk frame body: the chunk's position, then its bytes.
///
/// The index travels explicitly rather than being inferred from the frame sequence number, because
/// the scheduler reassigns chunks between lanes and a stalled chunk may arrive twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkFrame<'a> {
    index: ChunkIndex,
    payload: &'a [u8],
}

impl<'a> ChunkFrame<'a> {
    pub fn parse(body: &'a [u8]) -> Result<Self, WireError> {
        let truncated = || WireError::Truncated {
            needed: CHUNK_HEADER_LEN,
            available: body.len(),
        };
        let index = bytes::read_u32_le(body, 0).ok_or_else(truncated)?;
        let payload = body.get(CHUNK_HEADER_LEN..).ok_or_else(truncated)?;
        Ok(Self {
            index: ChunkIndex::new(index),
            payload,
        })
    }

    pub const fn index(&self) -> ChunkIndex {
        self.index
    }

    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

pub const fn chunk_header_bytes(index: ChunkIndex) -> [u8; CHUNK_HEADER_LEN] {
    index.as_u32().to_le_bytes()
}
