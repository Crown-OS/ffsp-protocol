//! What the two ends say to each other about a transfer.
//!
//! [`wire`](crate::wire) decides how a frame is shaped; this decides what goes inside one that
//! is not a chunk. Encoding is by hand and little-endian throughout, matching the rest of the
//! crate: these messages cross between a Rust engine and a Kotlin one, and a derived format is a
//! format neither side can see.

mod ack;
mod chunk_map;
mod error;
mod invite;
mod keepalive;
mod lane_report;
mod manifest_codec;
mod message;
mod opcode;
mod reason;
mod transfer_id;

pub use ack::{ChunkAck, Delivery};
pub use chunk_map::{CHUNK_MAP_PAGE_LEN, ChunkMapPage};
pub use error::ControlError;
pub use invite::LaneInvite;
pub use keepalive::KeepAlive;
pub use lane_report::LaneReport;
pub use manifest_codec::encode_manifest;
pub use message::ControlMessage;
pub use opcode::ControlOpcode;
pub use reason::EndReason;
pub use transfer_id::TransferId;

#[cfg(test)]
mod tests;
