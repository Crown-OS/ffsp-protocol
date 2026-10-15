/// Names one transfer within a session.
///
/// Sessions outlive transfers, and a cancel arriving after the next offer has already gone out
/// must not cancel the wrong one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransferId(u32);

impl TransferId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}
