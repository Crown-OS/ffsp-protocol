use thiserror::Error;

#[derive(Debug, Error)]
pub enum PairingError {
    #[error("the handshake pattern could not be built: {0}")]
    Pattern(String),

    #[error("the handshake message was rejected")]
    Rejected,

    #[error("it is not this side's turn to speak")]
    OutOfTurn,

    #[error("the handshake is not finished")]
    Unfinished,

    #[error("the peer presented a key of {0} bytes")]
    MalformedPeerKey(usize),

    #[error("the peer's key does not match the stored link")]
    WrongPeer,

    #[error("the buffer holds {capacity} bytes, the message needs more")]
    BufferTooSmall { capacity: usize },
}
