//! Moving chunk bytes between lanes and files without copying them.

mod buffer_pool;
mod chunk_buffer;
mod error;
mod positional;
mod seekability;

pub use buffer_pool::ChunkBufferPool;
pub use chunk_buffer::{
    ChunkBuffer, PAGE_ALIGNMENT, buffer_footprint, sealed_chunk_capacity, sealed_frame_capacity,
};
pub use error::IoError;
pub use positional::{PositionalRead, PositionalWrite};
pub use seekability::{reserve, supports_positional_write};

#[cfg(test)]
mod tests;
