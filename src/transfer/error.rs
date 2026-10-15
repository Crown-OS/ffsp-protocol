use thiserror::Error;

use crate::wire::ChunkIndex;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransferError {
    #[error("payload of {0} bytes needs more than {max} chunks", max = u32::MAX)]
    PayloadTooLarge(u64),

    #[error("chunk {index} is outside a payload of {count} chunks", index = index.as_u32())]
    ChunkOutOfRange { index: ChunkIndex, count: u32 },

    #[error(
        "chunk {index} should be {expected} bytes, got {actual}",
        index = index.as_u32()
    )]
    ChunkLengthMismatch {
        index: ChunkIndex,
        expected: u32,
        actual: usize,
    },

    #[error("chunk {index} failed verification", index = index.as_u32())]
    ChunkCorrupt { index: ChunkIndex },

    #[error("the chaining values do not fold to the manifest root")]
    RootMismatch,

    #[error("expected {expected} chaining values, got {actual}")]
    ChainingValueCountMismatch { expected: usize, actual: usize },

    #[error("subtree offset {0} is not chunk aligned")]
    UnalignedSubtree(u64),

    #[error("a subtree cannot be empty")]
    EmptySubtree,

    #[error("manifest entry name {0:?} would escape the destination directory")]
    UnsafeEntryName(String),

    #[error("a resume bitmap for {expected} chunks cannot be read from {actual} bytes")]
    BitmapLength { expected: usize, actual: usize },
}
