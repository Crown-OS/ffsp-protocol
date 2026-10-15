use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WireError {
    #[error("frame needs {needed} bytes, got {available}")]
    Truncated { needed: usize, available: usize },

    #[error("frame kind {0} is not known to this protocol version")]
    UnknownFrameKind(u8),

    #[error("body length {0} exceeds the {max} byte frame limit", max = super::MAX_BODY_LEN)]
    BodyTooLong(usize),

    #[error("protocol version {0} is not supported")]
    UnsupportedVersion(u8),
}
