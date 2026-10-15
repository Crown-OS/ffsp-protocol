/// Identifies one lane within a session for the lifetime of that session.
///
/// A lane's AEAD key and nonce space are derived from this value, so
/// [`LaneRegistry`](crate::medium::LaneRegistry) never reissues one: reuse would restart a nonce
/// counter under an existing key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LaneId(u16);

impl LaneId {
    /// Frames exchanged before any lane is registered, such as the handshake.
    pub const HANDSHAKE: Self = Self(0);
    pub const MAX: Self = Self(u16::MAX);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn as_u16(self) -> u16 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}
