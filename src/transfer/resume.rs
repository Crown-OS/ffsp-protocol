use blake3::Hash;

use crate::transfer::{ChunkBitmap, ChunkLayout, TransferError};
use crate::util::bytes;

/// Everything needed to continue a transfer after every lane died.
///
/// There is nothing here about lanes, offsets or partial chunks, because a chunk either verified
/// and was written or it did not happen. That is a consequence of per-chunk verification rather
/// than a feature that had to be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeState {
    root: Hash,
    total_len: u64,
    bitmap: ChunkBitmap,
}

impl ResumeState {
    const HEADER_LEN: usize = 40;

    pub const fn new(root: Hash, total_len: u64, bitmap: ChunkBitmap) -> Self {
        Self {
            root,
            total_len,
            bitmap,
        }
    }

    pub const fn root(&self) -> &Hash {
        &self.root
    }

    pub const fn total_len(&self) -> u64 {
        self.total_len
    }

    pub const fn bitmap(&self) -> &ChunkBitmap {
        &self.bitmap
    }

    pub fn layout(&self) -> Result<ChunkLayout, TransferError> {
        ChunkLayout::new(self.total_len)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(Self::HEADER_LEN + self.bitmap.to_bytes().len());
        out.extend_from_slice(self.root.as_bytes());
        out.extend_from_slice(&self.total_len.to_le_bytes());
        out.extend_from_slice(&self.bitmap.to_bytes());
        out
    }

    pub fn parse(source: &[u8]) -> Result<Self, TransferError> {
        let root: [u8; 32] = source
            .get(..32)
            .and_then(|slice| slice.try_into().ok())
            .ok_or(TransferError::BitmapLength {
                expected: Self::HEADER_LEN,
                actual: source.len(),
            })?;
        let total_len = bytes::read_u64_le(source, 32).ok_or(TransferError::BitmapLength {
            expected: Self::HEADER_LEN,
            actual: source.len(),
        })?;
        let words = source
            .get(Self::HEADER_LEN..)
            .ok_or(TransferError::BitmapLength {
                expected: Self::HEADER_LEN,
                actual: source.len(),
            })?;
        let layout = ChunkLayout::new(total_len)?;
        Ok(Self {
            root: Hash::from_bytes(root),
            total_len,
            bitmap: ChunkBitmap::from_bytes(words, layout.count())?,
        })
    }
}
