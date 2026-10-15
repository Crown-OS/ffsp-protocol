/// What the platform knows about the device's power situation.
///
/// This crate never reads a battery; it accepts a hint and narrows how many lanes it keeps warm
/// and how often it probes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PowerHint {
    /// Mains power: keep every useful lane open and probe freely.
    Unconstrained,
    #[default]
    /// On battery with charge to spare.
    Balanced,
    /// Low battery or thermal pressure: one lane, no probing.
    Conserve,
}

impl PowerHint {
    pub const fn max_bulk_lanes(self) -> usize {
        match self {
            Self::Unconstrained => 4,
            Self::Balanced => 2,
            Self::Conserve => 1,
        }
    }

    pub const fn allows_probing(self) -> bool {
        matches!(self, Self::Unconstrained | Self::Balanced)
    }
}
