//! Byte layout for FFSP.
//!
//! Allocation-free and I/O-free: every parser is a total function over `&[u8]` that either yields
//! a borrowed view or a [`WireError`], which makes the whole surface cheap to fuzz and impossible
//! to misuse from a hot loop.

mod chunk_frame;
mod chunk_index;
mod error;
mod frame_header;
mod frame_kind;
mod lane_id;
mod sequence;
mod version;

pub use chunk_frame::{ChunkFrame, chunk_header_bytes};
pub use chunk_index::ChunkIndex;
pub use error::WireError;
pub use frame_header::FrameHeader;
pub use frame_kind::FrameKind;
pub use lane_id::LaneId;
pub use sequence::SequenceNumber;
pub use version::ProtocolVersion;

/// Bytes an AEAD tag adds to every frame.
pub const TAG_LEN: usize = 16;

/// Bytes a chunk frame body spends on the chunk index before its payload.
pub const CHUNK_HEADER_LEN: usize = 4;

/// Largest body a single frame can carry, bounded by the 24-bit length field.
pub const MAX_BODY_LEN: usize = 0x00FF_FFFF;

#[cfg(test)]
mod tests;
