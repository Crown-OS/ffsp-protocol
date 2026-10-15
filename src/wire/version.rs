#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProtocolVersion(u8);

impl ProtocolVersion {
    pub const CURRENT: Self = Self(1);

    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    pub const fn as_byte(self) -> u8 {
        self.0
    }

    pub const fn is_supported(self) -> bool {
        self.0 == Self::CURRENT.0
    }
}
