use crate::transfer::LaneEstimator;
use crate::wire::LaneId;

/// A lane's outstanding work, as the assignment rule sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneLoad {
    pub lane: LaneId,
    pub queued_bytes: u64,
    pub estimator: LaneEstimator,
}

impl LaneLoad {
    /// Microseconds until this lane would finish a further `chunk_len` bytes.
    ///
    /// Half the round trip is added because the chunk is not done when it leaves; it is done when
    /// its acknowledgement could have returned.
    pub const fn completion_micros(&self, chunk_len: u32) -> u64 {
        let pending = self.queued_bytes.saturating_add(chunk_len as u64);
        self.estimator.drain_micros(pending) + self.estimator.rtt_micros() as u64 / 2
    }
}

/// Earliest completion first.
///
/// Picking the lane that finishes the chunk soonest, rather than the fastest lane or a round
/// robin, is what keeps a 15-times-slower lane useful without letting it hold up the transfer: it
/// receives work only while its queue stays short enough to still win.
pub fn earliest_completion(loads: &[LaneLoad], chunk_len: u32) -> Option<LaneId> {
    loads
        .iter()
        .min_by_key(|load| (load.completion_micros(chunk_len), load.lane))
        .map(|load| load.lane)
}
