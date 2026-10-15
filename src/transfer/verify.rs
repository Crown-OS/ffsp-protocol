use blake3::Hash;
use blake3::hazmat::{
    ChainingValue, HasherExt, Mode, left_subtree_len, merge_subtrees_non_root, merge_subtrees_root,
};

use crate::transfer::{CHUNK_LEN, ChunkLayout, TransferError};
use crate::wire::ChunkIndex;

/// Chaining value of one chunk, at the position it occupies in the payload.
///
/// The offset is what binds the chunk to its position: the same bytes at a different offset produce
/// a different chaining value, so a chunk cannot be replayed elsewhere in the payload.
pub fn chunk_chaining_value(offset: u64, payload: &[u8]) -> Result<ChainingValue, TransferError> {
    if payload.is_empty() {
        return Err(TransferError::EmptySubtree);
    }
    if !offset.is_multiple_of(u64::from(CHUNK_LEN)) {
        return Err(TransferError::UnalignedSubtree(offset));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.set_input_offset(offset);
    hasher.update(payload);
    Ok(hasher.finalize_non_root())
}

/// Fold per-chunk chaining values into the payload's root hash.
///
/// Rebuilds BLAKE3's own tree shape, so the result equals `blake3::hash` of the whole payload.
pub fn fold_root(
    chaining_values: &[ChainingValue],
    layout: &ChunkLayout,
) -> Result<Hash, TransferError> {
    let expected = layout.count() as usize;
    if chaining_values.len() != expected {
        return Err(TransferError::ChainingValueCountMismatch {
            expected,
            actual: chaining_values.len(),
        });
    }
    let (left, right) = split(chaining_values, layout.total_len())?;
    Ok(merge_subtrees_root(&left, &right, Mode::Hash))
}

fn fold_subtree(
    chaining_values: &[ChainingValue],
    len: u64,
) -> Result<ChainingValue, TransferError> {
    if let [only] = chaining_values {
        return Ok(*only);
    }
    let (left, right) = split(chaining_values, len)?;
    Ok(merge_subtrees_non_root(&left, &right, Mode::Hash))
}

fn split(
    chaining_values: &[ChainingValue],
    len: u64,
) -> Result<(ChainingValue, ChainingValue), TransferError> {
    if chaining_values.len() < 2 || len <= u64::from(CHUNK_LEN) {
        return Err(TransferError::EmptySubtree);
    }
    let left_len = left_subtree_len(len);
    if !left_len.is_multiple_of(u64::from(CHUNK_LEN)) {
        return Err(TransferError::UnalignedSubtree(left_len));
    }
    let boundary = usize::try_from(left_len / u64::from(CHUNK_LEN))
        .map_err(|_| TransferError::UnalignedSubtree(left_len))?;
    let left_values = chaining_values
        .get(..boundary)
        .ok_or(TransferError::EmptySubtree)?;
    let right_values = chaining_values
        .get(boundary..)
        .ok_or(TransferError::EmptySubtree)?;
    let right_len = len
        .checked_sub(left_len)
        .ok_or(TransferError::EmptySubtree)?;
    Ok((
        fold_subtree(left_values, left_len)?,
        fold_subtree(right_values, right_len)?,
    ))
}

/// Verifies arriving chunks against a manifest's root hash.
///
/// Construction is where trust is established: the chaining values are only accepted if they fold
/// to the root the sender committed to. After that, every chunk verifies on its own, so chunks may
/// arrive out of order, twice, or over different lanes at no cost.
#[derive(Debug, Clone)]
pub struct ChunkVerifier {
    root: Hash,
    layout: ChunkLayout,
    chaining_values: Vec<ChainingValue>,
}

impl ChunkVerifier {
    pub fn new(
        root: Hash,
        layout: ChunkLayout,
        chaining_values: Vec<ChainingValue>,
    ) -> Result<Self, TransferError> {
        if layout.is_single_subtree() {
            if !chaining_values.is_empty() {
                return Err(TransferError::ChainingValueCountMismatch {
                    expected: 0,
                    actual: chaining_values.len(),
                });
            }
        } else if fold_root(&chaining_values, &layout)? != root {
            return Err(TransferError::RootMismatch);
        }
        Ok(Self {
            root,
            layout,
            chaining_values,
        })
    }

    pub const fn layout(&self) -> &ChunkLayout {
        &self.layout
    }

    pub const fn root(&self) -> &Hash {
        &self.root
    }

    pub fn verify(&self, index: ChunkIndex, payload: &[u8]) -> Result<(), TransferError> {
        let expected_len = self
            .layout
            .len_at(index)
            .ok_or(TransferError::ChunkOutOfRange {
                index,
                count: self.layout.count(),
            })?;
        if payload.len() != expected_len as usize {
            return Err(TransferError::ChunkLengthMismatch {
                index,
                expected: expected_len,
                actual: payload.len(),
            });
        }

        if self.layout.is_single_subtree() {
            return if blake3::hash(payload) == self.root {
                Ok(())
            } else {
                Err(TransferError::ChunkCorrupt { index })
            };
        }

        let offset = self
            .layout
            .byte_offset(index)
            .ok_or(TransferError::ChunkOutOfRange {
                index,
                count: self.layout.count(),
            })?;
        let expected =
            self.chaining_values
                .get(index.as_usize())
                .ok_or(TransferError::ChunkOutOfRange {
                    index,
                    count: self.layout.count(),
                })?;
        if chunk_chaining_value(offset, payload)? == *expected {
            Ok(())
        } else {
            Err(TransferError::ChunkCorrupt { index })
        }
    }
}

/// Chaining values for a payload already in memory. The streaming sender computes these per chunk
/// as it reads, rather than materialising the payload.
pub fn chaining_values_of(
    payload: &[u8],
    layout: &ChunkLayout,
) -> Result<Vec<ChainingValue>, TransferError> {
    if layout.is_single_subtree() {
        return Ok(Vec::new());
    }
    layout
        .indices()
        .map(|index| {
            let offset = layout
                .byte_offset(index)
                .ok_or(TransferError::ChunkOutOfRange {
                    index,
                    count: layout.count(),
                })?;
            let len = layout.len_at(index).ok_or(TransferError::ChunkOutOfRange {
                index,
                count: layout.count(),
            })?;
            let start =
                usize::try_from(offset).map_err(|_| TransferError::UnalignedSubtree(offset))?;
            let end = start
                .checked_add(len as usize)
                .ok_or(TransferError::UnalignedSubtree(offset))?;
            let slice = payload
                .get(start..end)
                .ok_or(TransferError::ChunkOutOfRange {
                    index,
                    count: layout.count(),
                })?;
            chunk_chaining_value(offset, slice)
        })
        .collect()
}
