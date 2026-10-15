use std::borrow::Cow;

use crate::medium::Medium;
use crate::metrics::{LaneMeter, TransferRole};
use crate::transfer::LaneStats;
use crate::wire::LaneId;

/// One lane's line in the report, with both ends' views of it side by side.
///
/// They disagree on purpose. `mean_kbps` is what the receiver watched arrive; `scheduler_kbps` is
/// what the sender inferred from acknowledgements coming back. A gap between them is not an error
/// to be averaged away — it is the cost of the acknowledgement path, and worth seeing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSummary {
    pub lane: LaneId,
    pub medium: Medium,
    pub endpoint: Cow<'static, str>,
    pub open_micros: u64,

    /// Payload bytes this lane moved, in whichever direction this end was moving them.
    pub carried_bytes: u64,
    pub carried_chunks: u64,
    pub received_bytes: u64,
    pub received_chunks: u64,
    pub sent_bytes: u64,
    pub sent_chunks: u64,
    pub frames_in: u64,
    pub frames_out: u64,
    pub duplicate_bytes: u64,

    pub mean_kbps: u32,
    pub peak_kbps: u32,

    pub rtt_min_micros: Option<u32>,
    pub rtt_mean_micros: Option<u32>,
    pub rtt_p50_micros: Option<u32>,
    pub rtt_p95_micros: Option<u32>,
    pub rtt_max_micros: Option<u32>,
    pub rtt_samples: usize,

    pub scheduler_kbps: u32,
    pub scheduler_rtt_micros: u32,
    pub stalls: u32,
    pub duplicate_wins: u32,
    pub park_events: usize,
    pub parked: bool,
}

impl LaneSummary {
    pub fn new(
        meter: &LaneMeter,
        stats: Option<&LaneStats>,
        now_micros: u64,
        role: TransferRole,
    ) -> Self {
        let rtt = meter.rtt();
        Self {
            carried_bytes: meter.carried_bytes(role),
            carried_chunks: meter.carried_chunks(role),
            lane: meter.lane(),
            medium: meter.medium(),
            endpoint: Cow::Owned(meter.endpoint().to_owned()),
            open_micros: meter.open_micros(now_micros),
            received_bytes: meter.received_bytes(),
            received_chunks: meter.received_chunks(),
            sent_bytes: meter.sent_bytes(),
            sent_chunks: meter.sent_chunks(),
            frames_in: meter.frames_in(),
            frames_out: meter.frames_out(),
            duplicate_bytes: meter.duplicate_bytes(),
            mean_kbps: meter.mean_kbps(now_micros, role),
            peak_kbps: meter.peak_kbps(),
            rtt_min_micros: rtt.min(),
            rtt_mean_micros: rtt.mean(),
            rtt_p50_micros: rtt.percentile(50),
            rtt_p95_micros: rtt.percentile(95),
            rtt_max_micros: rtt.max(),
            rtt_samples: rtt.count(),
            scheduler_kbps: stats.map(|s| s.bandwidth_kbps).unwrap_or_default(),
            scheduler_rtt_micros: stats.map(|s| s.rtt_micros).unwrap_or_default(),
            stalls: stats.map(|s| s.stalls).unwrap_or_default(),
            duplicate_wins: stats.map(|s| s.duplicate_wins).unwrap_or_default(),
            park_events: meter.parks().len(),
            parked: stats.is_some_and(|s| s.parked),
        }
    }

    /// Share of the transfer this lane carried, in tenths of a percent.
    pub fn share_permille(&self, total_bytes: u64) -> u32 {
        if total_bytes == 0 {
            return 0;
        }
        let share = self.carried_bytes.saturating_mul(1_000) / total_bytes;
        u32::try_from(share).unwrap_or(u32::MAX)
    }
}
