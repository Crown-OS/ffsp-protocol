use crate::transfer::TransferError;
use crate::wire::ChunkIndex;

const BITS_PER_WORD: u32 = u64::BITS;

/// Which chunks have arrived.
///
/// One bit per chunk, so a 10 GB transfer costs 1280 bytes. Persisting it is the whole of resume:
/// because chunks verify independently, a restart needs no other state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkBitmap {
    words: Box<[u64]>,
    count: u32,
    completed: u32,
}

impl ChunkBitmap {
    pub fn new(count: u32) -> Self {
        let words = count.div_ceil(BITS_PER_WORD) as usize;
        Self {
            words: vec![0; words].into_boxed_slice(),
            count,
            completed: 0,
        }
    }

    /// Records a chunk as present, returning whether this was new.
    ///
    /// A chunk arriving twice is normal: a stalled chunk is reassigned while the original may
    /// still be in flight, and the tail is sent twice deliberately.
    pub fn insert(&mut self, index: ChunkIndex) -> bool {
        let Some((word, mask)) = self.locate(index) else {
            return false;
        };
        let Some(slot) = self.words.get_mut(word) else {
            return false;
        };
        if *slot & mask != 0 {
            return false;
        }
        *slot |= mask;
        self.completed += 1;
        true
    }

    pub fn contains(&self, index: ChunkIndex) -> bool {
        self.locate(index)
            .and_then(|(word, mask)| self.words.get(word).map(|slot| slot & mask != 0))
            .unwrap_or(false)
    }

    pub const fn count(&self) -> u32 {
        self.count
    }

    pub const fn completed(&self) -> u32 {
        self.completed
    }

    pub const fn remaining(&self) -> u32 {
        self.count - self.completed
    }

    pub const fn is_complete(&self) -> bool {
        self.completed == self.count
    }

    pub fn missing(&self) -> impl Iterator<Item = ChunkIndex> {
        (0..self.count)
            .map(ChunkIndex::new)
            .filter(|index| !self.contains(*index))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }

    pub fn from_bytes(bytes: &[u8], count: u32) -> Result<Self, TransferError> {
        let expected = count.div_ceil(BITS_PER_WORD) as usize * size_of::<u64>();
        if bytes.len() != expected {
            return Err(TransferError::BitmapLength {
                expected,
                actual: bytes.len(),
            });
        }
        let words: Box<[u64]> = bytes
            .chunks_exact(size_of::<u64>())
            .filter_map(|chunk| <[u8; 8]>::try_from(chunk).ok())
            .map(u64::from_le_bytes)
            .collect();

        let mut bitmap = Self {
            words,
            count,
            completed: 0,
        };
        bitmap.completed = (0..count)
            .map(ChunkIndex::new)
            .filter(|index| bitmap.contains(*index))
            .count()
            .try_into()
            .unwrap_or(count);
        Ok(bitmap)
    }

    const fn locate(&self, index: ChunkIndex) -> Option<(usize, u64)> {
        let raw = index.as_u32();
        if raw >= self.count {
            return None;
        }
        Some((
            (raw / BITS_PER_WORD) as usize,
            1u64 << (raw % BITS_PER_WORD),
        ))
    }
}
