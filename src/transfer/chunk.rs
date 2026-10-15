use crate::transfer::TransferError;
use crate::wire::ChunkIndex;

/// The unit the scheduler moves, and one BLAKE3 subtree.
///
/// 1 MiB is 1024 BLAKE3 chunks, so it is a power-of-two multiple of BLAKE3's own 1024-byte chunk
/// and lands on a valid subtree boundary at every 1 MiB-aligned offset. That is what lets a chunk
/// be verified on its own, in any order, without its neighbours.
pub const CHUNK_LEN: u32 = 1 << 20;

/// How a payload of a given length divides into chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLayout {
    total_len: u64,
    count: u32,
}

impl ChunkLayout {
    pub fn new(total_len: u64) -> Result<Self, TransferError> {
        let chunk_len = u64::from(CHUNK_LEN);
        let count = total_len.div_ceil(chunk_len);
        let count = u32::try_from(count).map_err(|_| TransferError::PayloadTooLarge(total_len))?;
        Ok(Self { total_len, count })
    }

    pub const fn total_len(&self) -> u64 {
        self.total_len
    }

    pub const fn count(&self) -> u32 {
        self.count
    }

    /// A payload of one chunk or less has no parent nodes, so its root hash is the chunk's own.
    pub const fn is_single_subtree(&self) -> bool {
        self.count <= 1
    }

    pub const fn byte_offset(&self, index: ChunkIndex) -> Option<u64> {
        if index.as_u32() >= self.count {
            return None;
        }
        Some(index.byte_offset(CHUNK_LEN))
    }

    /// Length of one chunk; every chunk is [`CHUNK_LEN`] except possibly the last.
    pub fn len_at(&self, index: ChunkIndex) -> Option<u32> {
        let offset = self.byte_offset(index)?;
        let remaining = self.total_len.checked_sub(offset)?;
        u32::try_from(remaining.min(u64::from(CHUNK_LEN))).ok()
    }

    pub fn indices(&self) -> impl Iterator<Item = ChunkIndex> {
        (0..self.count).map(ChunkIndex::new)
    }
}
