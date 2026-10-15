use crate::io::{buffer_footprint, sealed_frame_capacity};
use crate::platform::PowerHint;
use crate::transfer::{CHUNK_LEN, ChunkScheduler};

/// Knobs the platform sets, mostly from how much power it is willing to spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    pub max_bulk_lanes: usize,
    pub max_inflight_per_lane: u32,
    pub buffer_pool_size: usize,
    pub probing: bool,
    pub presence_ttl_millis: u64,
}

impl Tuning {
    pub const fn for_power(hint: PowerHint) -> Self {
        let max_bulk_lanes = hint.max_bulk_lanes();
        Self {
            max_bulk_lanes,
            max_inflight_per_lane: ChunkScheduler::DEFAULT_MAX_INFLIGHT_PER_LANE,
            buffer_pool_size: max_bulk_lanes
                * ChunkScheduler::DEFAULT_MAX_INFLIGHT_PER_LANE as usize
                * 2,
            probing: hint.allows_probing(),
            presence_ttl_millis: crate::discovery::PresenceTracker::DEFAULT_TTL_MILLIS,
        }
    }

    /// Bytes the buffer pool will hold. This is the transfer's memory ceiling, whatever the
    /// payload size.
    ///
    /// Counted as the buffers are actually allocated — sized for a whole received frame, and
    /// padded so each can align itself inside its own storage.
    pub const fn buffer_footprint(&self) -> usize {
        self.buffer_pool_size * buffer_footprint(sealed_frame_capacity(CHUNK_LEN))
    }

    /// Capacity each buffer in the pool is created with.
    pub const fn buffer_capacity(&self) -> usize {
        sealed_frame_capacity(CHUNK_LEN)
    }
}

impl Default for Tuning {
    fn default() -> Self {
        Self::for_power(PowerHint::Balanced)
    }
}
