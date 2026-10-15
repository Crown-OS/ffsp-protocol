use crate::medium::Medium;
use crate::wire::LaneId;

/// One lane's throughput over one window.
///
/// Integer kilobits throughout, like [`LaneEstimator`](crate::transfer::LaneEstimator): these
/// travel in [`EngineEvent`](crate::engine::EngineEvent), which is comparable, and a float would
/// take that away for no accuracy anyone can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThroughputSample {
    pub at_micros: u64,
    pub lane: LaneId,
    pub medium: Medium,
    /// Bytes this lane had carried by the end of the window.
    pub cumulative_bytes: u64,
    /// Rate over the window alone, not since the start.
    pub kbps: u32,
}

/// Round trips measured on one lane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RttSummary {
    samples: Vec<u32>,
}

impl RttSummary {
    /// Keepalives arrive about once a second, so a transfer of any interesting length fits.
    /// Past that the oldest go, since the recent picture is the one worth having.
    const CAPACITY: usize = 4096;

    pub fn observe(&mut self, micros: u32) {
        if self.samples.len() == Self::CAPACITY {
            self.samples.remove(0);
        }
        self.samples.push(micros);
    }

    pub const fn count(&self) -> usize {
        self.samples.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn min(&self) -> Option<u32> {
        self.samples.iter().copied().min()
    }

    pub fn max(&self) -> Option<u32> {
        self.samples.iter().copied().max()
    }

    pub fn mean(&self) -> Option<u32> {
        if self.samples.is_empty() {
            return None;
        }
        let total: u64 = self.samples.iter().map(|micros| u64::from(*micros)).sum();
        u32::try_from(total / self.samples.len() as u64).ok()
    }

    /// Nearest-rank percentile, which needs no interpolation and so no floating point.
    pub fn percentile(&self, percent: u32) -> Option<u32> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let rank = (percent.min(100) as usize * sorted.len()).div_ceil(100);
        sorted.get(rank.saturating_sub(1)).copied()
    }
}
