use aes_gcm::aead::Buffer;
use aes_gcm::aead::generic_array::typenum::Unsigned;

use crate::io::IoError;

/// Alignment that satisfies a 4 KiB and a 16 KiB page alike.
///
/// Android 15 and later run with 16 KiB pages on some devices, so querying the kernel would mean
/// carrying the answer through every allocation. Aligning to the larger of the two is correct on
/// both and costs one comparison.
pub const PAGE_ALIGNMENT: usize = 16 * 1024;

/// A page-aligned buffer sized for one sealed chunk.
///
/// Alignment is what lets the same buffer be handed to positional reads and writes without the
/// kernel bouncing it through an intermediate copy.
#[derive(Debug)]
pub struct ChunkBuffer {
    storage: Box<[u8]>,
    offset: usize,
    capacity: usize,
    len: usize,
}

impl ChunkBuffer {
    pub fn with_capacity(capacity: usize) -> Self {
        let storage = vec![0u8; capacity + PAGE_ALIGNMENT].into_boxed_slice();
        let offset = alignment_padding(storage.as_ptr() as usize, PAGE_ALIGNMENT);
        Self {
            storage,
            offset,
            capacity,
            len: 0,
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn clear(&mut self) {
        self.len = 0;
    }

    pub fn is_page_aligned(&self) -> bool {
        (self.as_slice().as_ptr() as usize).is_multiple_of(PAGE_ALIGNMENT)
    }

    pub fn as_slice(&self) -> &[u8] {
        self.storage
            .get(self.offset..self.offset + self.len)
            .unwrap_or_default()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let range = self.offset..self.offset + self.len;
        self.storage.get_mut(range).unwrap_or_default()
    }

    /// The unwritten remainder, for filling from a read.
    pub fn spare_capacity_mut(&mut self) -> &mut [u8] {
        let range = self.offset + self.len..self.offset + self.capacity;
        self.storage.get_mut(range).unwrap_or_default()
    }

    /// Declares how many bytes of the spare capacity were filled.
    pub const fn commit(&mut self, additional: usize) -> Result<(), IoError> {
        match self.len.checked_add(additional) {
            Some(len) if len <= self.capacity => {
                self.len = len;
                Ok(())
            }
            _ => Err(IoError::BufferTooSmall {
                capacity: self.capacity,
                requested: self.len + additional,
            }),
        }
    }

    pub fn fill_from(&mut self, source: &[u8]) -> Result<(), IoError> {
        self.clear();
        Buffer::extend_from_slice(self, source).map_err(|_| IoError::BufferTooSmall {
            capacity: self.capacity,
            requested: source.len(),
        })
    }
}

impl AsRef<[u8]> for ChunkBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for ChunkBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

/// Lets the AEAD seal straight into the aligned buffer instead of a temporary `Vec`.
impl Buffer for ChunkBuffer {
    fn extend_from_slice(&mut self, other: &[u8]) -> aes_gcm::aead::Result<()> {
        let start = self.offset + self.len;
        let end = start.checked_add(other.len()).ok_or(aes_gcm::aead::Error)?;
        if self.len + other.len() > self.capacity {
            return Err(aes_gcm::aead::Error);
        }
        self.storage
            .get_mut(start..end)
            .ok_or(aes_gcm::aead::Error)?
            .copy_from_slice(other);
        self.len += other.len();
        Ok(())
    }

    fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }
}

/// Capacity one sealed chunk needs: the payload, its chunk header and the authentication tag.
pub const fn sealed_chunk_capacity(chunk_len: u32) -> usize {
    chunk_len as usize
        + crate::wire::CHUNK_HEADER_LEN
        + <aes_gcm::Aes256Gcm as aes_gcm::AeadCore>::TagSize::USIZE
}

/// Capacity a whole received frame needs: the sealed chunk and the frame header in front of it.
///
/// A receiver reads both into one buffer, so a pool sized only for the sealed chunk is eight
/// bytes short of every frame it will ever be handed.
pub const fn sealed_frame_capacity(chunk_len: u32) -> usize {
    crate::wire::FrameHeader::LEN + sealed_chunk_capacity(chunk_len)
}

/// Bytes a buffer of this capacity actually occupies.
///
/// [`ChunkBuffer::with_capacity`] over-allocates by a page so it can align inside its own
/// storage, and a memory ceiling that ignored that would under-report by that page per buffer.
pub const fn buffer_footprint(capacity: usize) -> usize {
    capacity + PAGE_ALIGNMENT
}

const fn alignment_padding(address: usize, alignment: usize) -> usize {
    match address % alignment {
        0 => 0,
        misalignment => alignment - misalignment,
    }
}
