use thiserror::Error;

use crate::medium::Medium;
use crate::wire::{LaneId, WireError};

#[derive(Debug, Error)]
pub enum MediumError {
    #[error("medium byte {0} is not known to this protocol version")]
    UnknownMedium(u8),

    #[error("{0} is not available on this device")]
    Unavailable(Medium),

    #[error("the operating system denied access to {medium}: {reason}")]
    PermissionDenied { medium: Medium, reason: String },

    #[error("no peer reachable over {0}")]
    PeerUnreachable(Medium),

    #[error("opening a {medium} lane failed: {source}")]
    Io {
        medium: Medium,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Error)]
pub enum LaneError {
    #[error("the lane closed")]
    Closed,

    #[error("frame of {len} bytes exceeds the lane's {max} byte limit")]
    FrameTooLong { len: usize, max: usize },

    #[error(transparent)]
    Wire(#[from] WireError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LaneRegistryError {
    /// Reissuing a [`LaneId`](crate::wire::LaneId) would restart a nonce counter under a key that
    /// has already used it, so the session ends instead.
    #[error("this session has exhausted its lane identifiers")]
    LaneIdsExhausted,

    #[error("no lane is registered for this session that can carry control frames")]
    NoControlLane,

    /// Adopting an identifier the allocator has already passed would pair a fresh nonce counter
    /// with a key that has used those nonces.
    #[error("lane identifier {lane:?} has already been issued in this session")]
    LaneAlreadyIssued { lane: LaneId },
}
