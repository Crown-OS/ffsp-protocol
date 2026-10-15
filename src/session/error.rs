use thiserror::Error;

use crate::crypto::CryptoError;
use crate::medium::LaneRegistryError;
use crate::session::{SessionSignal, SessionState};
use crate::wire::LaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SessionError {
    #[error("lane {lane} is not part of this session", lane = lane.as_u16())]
    UnknownLane { lane: LaneId },

    #[error("a session that is {state:?} cannot handle {signal:?}")]
    IllegalTransition {
        state: SessionState,
        signal: SessionSignal,
    },

    #[error("a session that is {0:?} cannot move data")]
    NotTransferable(SessionState),

    #[error(transparent)]
    Crypto(#[from] CryptoError),

    #[error(transparent)]
    Registry(#[from] LaneRegistryError),
}
