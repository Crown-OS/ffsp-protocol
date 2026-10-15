use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("a chunk buffer of {capacity} bytes cannot hold {requested}")]
    BufferTooSmall { capacity: usize, requested: usize },

    #[error("the destination does not support writing at an offset")]
    NotPositional,

    #[error("wrote {written} of {expected} bytes at offset {offset}")]
    ShortWrite {
        offset: u64,
        written: usize,
        expected: usize,
    },

    #[error("read {read} of {expected} bytes at offset {offset}")]
    ShortRead {
        offset: u64,
        read: usize,
        expected: usize,
    },

    #[error(transparent)]
    Os(#[from] std::io::Error),
}
