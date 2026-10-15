use thiserror::Error;

use crate::wire::SequenceNumber;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CryptoError {
    #[error(transparent)]
    Wire(#[from] crate::wire::WireError),

    #[error("cipher suite byte {0} is not known to this protocol version")]
    UnknownCipherSuite(u8),

    #[error("the frame did not authenticate")]
    NotAuthentic,

    #[error("a frame of {0} bytes is too short to hold an authentication tag")]
    MissingTag(usize),

    #[error(
        "frame {received} arrived where {expected} was expected",
        received = received.as_u16(),
        expected = expected.as_u16()
    )]
    SequenceDesync {
        expected: SequenceNumber,
        received: SequenceNumber,
    },

    /// Continuing would reuse a nonce under a key that has already used it.
    #[error("this lane has exhausted its nonce space")]
    NonceSpaceExhausted,
}
