use crate::wire::LaneId;

/// What the scheduler should do about a lane under measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAction {
    /// Nothing to do yet.
    Wait,
    /// Stop sending on this lane so its absence can be measured.
    SuspendForMeasurement(LaneId),
    /// The lane was earning its place; put it back to work.
    Restore(LaneId),
    /// Twice in a row, the transfer went faster without it.
    Park(LaneId),
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Idle {
        next_probe_millis: u64,
    },
    Baseline {
        lane: LaneId,
        ends_millis: u64,
        bytes_at_start: u64,
    },
    Suspended {
        lane: LaneId,
        ends_millis: u64,
        bytes_at_start: u64,
        baseline_bytes: u64,
    },
}

/// Decides whether a lane is making the transfer faster or slower.
///
/// Adding a lane is not always an improvement: two lanes on one radio split its airtime, and
/// Bluetooth alongside 2.4 GHz Wi-Fi actively interferes. Rather than trusting a table of which
/// combinations are safe, this measures the aggregate with and without the lane and believes the
/// result.
///
/// Two consecutive verdicts are required before parking, because a single window can be spoiled by
/// a disk stall, thermal throttling or someone walking between the devices.
#[derive(Debug)]
pub struct MarginalContributionProbe {
    window_millis: u64,
    interval_millis: u64,
    threshold_permille: u64,
    phase: Phase,
    consecutive_park_verdicts: u8,
}

impl MarginalContributionProbe {
    pub const DEFAULT_WINDOW_MILLIS: u64 = 1_000;
    pub const DEFAULT_INTERVAL_MILLIS: u64 = 5_000;
    /// A lane must cost more than five percent of aggregate throughput to be worth parking.
    pub const DEFAULT_THRESHOLD_PERMILLE: u64 = 50;
    const VERDICTS_BEFORE_PARKING: u8 = 2;

    pub const fn new(started_millis: u64) -> Self {
        Self {
            window_millis: Self::DEFAULT_WINDOW_MILLIS,
            interval_millis: Self::DEFAULT_INTERVAL_MILLIS,
            threshold_permille: Self::DEFAULT_THRESHOLD_PERMILLE,
            phase: Phase::Idle {
                next_probe_millis: started_millis + Self::DEFAULT_INTERVAL_MILLIS,
            },
            consecutive_park_verdicts: 0,
        }
    }

    /// Drives the measurement.
    ///
    /// `delivered_bytes` is the transfer's running total across every lane, and `candidate` is the
    /// lane the scheduler is willing to lose. Passing `None` keeps the probe idle.
    pub const fn poll(
        &mut self,
        now_millis: u64,
        delivered_bytes: u64,
        candidate: Option<LaneId>,
    ) -> ProbeAction {
        match self.phase {
            Phase::Idle { next_probe_millis } => {
                let Some(lane) = candidate else {
                    return ProbeAction::Wait;
                };
                if now_millis < next_probe_millis {
                    return ProbeAction::Wait;
                }
                self.phase = Phase::Baseline {
                    lane,
                    ends_millis: now_millis + self.window_millis,
                    bytes_at_start: delivered_bytes,
                };
                ProbeAction::Wait
            }

            Phase::Baseline {
                lane,
                ends_millis,
                bytes_at_start,
            } => {
                if now_millis < ends_millis {
                    return ProbeAction::Wait;
                }
                self.phase = Phase::Suspended {
                    lane,
                    ends_millis: now_millis + self.window_millis,
                    bytes_at_start: delivered_bytes,
                    baseline_bytes: delivered_bytes.saturating_sub(bytes_at_start),
                };
                ProbeAction::SuspendForMeasurement(lane)
            }

            Phase::Suspended {
                lane,
                ends_millis,
                bytes_at_start,
                baseline_bytes,
            } => {
                if now_millis < ends_millis {
                    return ProbeAction::Wait;
                }
                let without_lane = delivered_bytes.saturating_sub(bytes_at_start);
                self.phase = Phase::Idle {
                    next_probe_millis: now_millis + self.interval_millis,
                };

                if self.is_harmful(baseline_bytes, without_lane) {
                    self.consecutive_park_verdicts += 1;
                    if self.consecutive_park_verdicts >= Self::VERDICTS_BEFORE_PARKING {
                        self.consecutive_park_verdicts = 0;
                        return ProbeAction::Park(lane);
                    }
                } else {
                    self.consecutive_park_verdicts = 0;
                }
                ProbeAction::Restore(lane)
            }
        }
    }

    const fn is_harmful(&self, with_lane: u64, without_lane: u64) -> bool {
        let threshold = with_lane.saturating_mul(1_000 + self.threshold_permille) / 1_000;
        without_lane > threshold
    }

    pub const fn is_measuring(&self) -> bool {
        matches!(self.phase, Phase::Suspended { .. })
    }
}
