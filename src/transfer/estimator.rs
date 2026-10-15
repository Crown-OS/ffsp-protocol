use crate::medium::LaneProfile;

/// Smoothed view of what a lane is actually delivering.
///
/// Exponentially weighted so a single slow frame does not evict a lane, but a sustained collapse
/// moves the estimate within a few chunks. Integer arithmetic throughout: this runs on the
/// assignment path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneEstimator {
    bandwidth_kbps: u32,
    rtt_micros: u32,
}

impl LaneEstimator {
    /// Weight of a new sample, as a reciprocal: a quarter.
    const SMOOTHING_DIVISOR: u64 = 4;

    pub const fn from_profile(profile: &LaneProfile) -> Self {
        Self {
            bandwidth_kbps: profile.bandwidth_hint_kbps,
            rtt_micros: profile.rtt_hint_micros,
        }
    }

    pub fn observe_delivery(&mut self, bytes: u64, elapsed_micros: u64) {
        if elapsed_micros == 0 {
            return;
        }
        let sample = bytes.saturating_mul(8_000) / elapsed_micros;
        self.bandwidth_kbps = smooth(u64::from(self.bandwidth_kbps), sample).max(1);
    }

    pub fn observe_rtt(&mut self, micros: u32) {
        self.rtt_micros = smooth(u64::from(self.rtt_micros), u64::from(micros));
    }

    pub const fn bandwidth_kbps(&self) -> u32 {
        self.bandwidth_kbps
    }

    pub const fn rtt_micros(&self) -> u32 {
        self.rtt_micros
    }

    /// Microseconds this lane needs to drain `bytes`.
    pub const fn drain_micros(&self, bytes: u64) -> u64 {
        let kbps = if self.bandwidth_kbps == 0 {
            1
        } else {
            self.bandwidth_kbps
        };
        bytes.saturating_mul(8_000) / kbps as u64
    }
}

fn smooth(previous: u64, sample: u64) -> u32 {
    let divisor = LaneEstimator::SMOOTHING_DIVISOR;
    let blended = (sample + previous.saturating_mul(divisor - 1)) / divisor;
    u32::try_from(blended).unwrap_or(u32::MAX)
}
