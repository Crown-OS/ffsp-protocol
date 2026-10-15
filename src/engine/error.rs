use thiserror::Error;

use crate::control::ControlError;
use crate::crypto::CryptoError;
use crate::device::DeviceId;
use crate::io::IoError;
use crate::link::{LinkStoreError, PairingError};
use crate::medium::{LaneError, MediumError};
use crate::session::SessionError;
use crate::transfer::TransferError;
use crate::wire::{LaneId, WireError};

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("no session with {0}")]
    UnknownPeer(DeviceId),

    #[error("no transfer is in progress with {0}")]
    NoTransfer(DeviceId),

    #[error("lane {lane:?} is not attached")]
    UnknownLane { lane: LaneId },

    #[error("the engine has stopped")]
    Stopped,

    #[error("a handshake message of {len} bytes exceeds the {max} byte limit")]
    HandshakeTooLong { len: usize, max: usize },

    #[error("a {len} byte chunk does not fit a {capacity} byte buffer")]
    ChunkTooLong { len: usize, capacity: usize },

    #[error("chunk {index:?} lies outside the payload")]
    ChunkOutOfRange { index: crate::wire::ChunkIndex },

    #[error("the peer answered a handshake with a {0:?} frame")]
    UnexpectedFrame(crate::wire::FrameKind),

    #[error(transparent)]
    Pairing(#[from] PairingError),

    #[error(transparent)]
    LinkStore(#[from] LinkStoreError),

    #[error(transparent)]
    Session(#[from] SessionError),

    #[error(transparent)]
    Crypto(#[from] CryptoError),

    #[error(transparent)]
    Control(#[from] ControlError),

    #[error(transparent)]
    Transfer(#[from] TransferError),

    #[error(transparent)]
    Lane(#[from] LaneError),

    #[error(transparent)]
    Medium(#[from] MediumError),

    #[error(transparent)]
    Wire(#[from] WireError),

    #[error(transparent)]
    Io(#[from] IoError),

    #[error(transparent)]
    Os(#[from] std::io::Error),
}
