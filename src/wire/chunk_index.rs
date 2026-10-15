/// Position of a chunk within a payload, counted in whole chunks from the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkIndex(u32);

impl ChunkIndex {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    /// Byte offset of this chunk's first byte, given the payload's chunk length.
    pub const fn byte_offset(self, chunk_len: u32) -> u64 {
        self.0 as u64 * chunk_len as u64
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}
