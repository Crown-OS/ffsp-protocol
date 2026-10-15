/// Per-lane frame counter. Wraps, and is only ever compared for adjacency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceNumber(u16);

impl SequenceNumber {
    pub const ZERO: Self = Self(0);

    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    pub const fn as_u16(self) -> u16 {
        self.0
    }

    pub const fn wrapping_next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}
