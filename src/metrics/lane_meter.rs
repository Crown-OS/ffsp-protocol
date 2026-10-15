use std::borrow::Cow;

use crate::medium::Medium;
use crate::metrics::{RttSummary, ThroughputSample, TransferRole};
use crate::wire::LaneId;

/// When a lane stopped or resumed carrying work, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParkEvent {
    pub at_micros: u64,
    pub parked: bool,
}

/// What one lane did, measured where the bytes actually crossed.
///
/// The receiving end owns the truth here. A sender only learns of an arrival when the
/// acknowledgement returns, so its view of a lane is smeared by whatever path the acknowledgement
/// took; the receiver timestamps the frame as it lands.
#[derive(Debug, Clone)]
pub struct LaneMeter {
    lane: LaneId,
    medium: Medium,
    endpoint: Cow<'static, str>,
    opened_at_micros: u64,
    closed_at_micros: Option<u64>,
    received_bytes: u64,
    received_chunks: u64,
    sent_bytes: u64,
    sent_chunks: u64,
    frames_in: u64,
    frames_out: u64,
    /// Bytes that arrived for a chunk another lane had already delivered.
    duplicate_bytes: u64,
    rtt: RttSummary,
    parks: Vec<ParkEvent>,
    peak_kbps: u32,
    window_started_micros: u64,
    window_start_bytes: u64,
}

impl LaneMeter {
    pub fn new(
        lane: LaneId,
        medium: Medium,
        endpoint: impl Into<Cow<'static, str>>,
        at_micros: u64,
    ) -> Self {
        Self {
            lane,
            medium,
            endpoint: endpoint.into(),
            opened_at_micros: at_micros,
            closed_at_micros: None,
            received_bytes: 0,
            received_chunks: 0,
            sent_bytes: 0,
            sent_chunks: 0,
            frames_in: 0,
            frames_out: 0,
            duplicate_bytes: 0,
            rtt: RttSummary::default(),
            parks: Vec::new(),
            peak_kbps: 0,
            window_started_micros: at_micros,
            window_start_bytes: 0,
        }
    }

    pub const fn lane(&self) -> LaneId {
        self.lane
    }

    pub const fn medium(&self) -> Medium {
        self.medium
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub const fn received_bytes(&self) -> u64 {
        self.received_bytes
    }

    pub const fn received_chunks(&self) -> u64 {
        self.received_chunks
    }

    pub const fn sent_bytes(&self) -> u64 {
        self.sent_bytes
    }

    pub const fn sent_chunks(&self) -> u64 {
        self.sent_chunks
    }

    pub const fn frames_in(&self) -> u64 {
        self.frames_in
    }

    pub const fn frames_out(&self) -> u64 {
        self.frames_out
    }

    pub const fn duplicate_bytes(&self) -> u64 {
        self.duplicate_bytes
    }

    pub const fn peak_kbps(&self) -> u32 {
        self.peak_kbps
    }

    pub const fn rtt(&self) -> &RttSummary {
        &self.rtt
    }

    pub fn parks(&self) -> &[ParkEvent] {
        &self.parks
    }

    pub const fn record_received_frame(&mut self, bytes: u64) {
        self.frames_in = self.frames_in.saturating_add(1);
        self.received_bytes = self.received_bytes.saturating_add(bytes);
    }

    /// A chunk that verified and was written, as opposed to one that arrived a second time.
    pub const fn record_received_chunk(&mut self, payload_bytes: u64, duplicate: bool) {
        if duplicate {
            self.duplicate_bytes = self.duplicate_bytes.saturating_add(payload_bytes);
        } else {
            self.received_chunks = self.received_chunks.saturating_add(1);
        }
    }

    pub const fn record_sent_frame(&mut self, bytes: u64) {
        self.frames_out = self.frames_out.saturating_add(1);
        self.sent_bytes = self.sent_bytes.saturating_add(bytes);
    }

    pub const fn record_sent_chunk(&mut self) {
        self.sent_chunks = self.sent_chunks.saturating_add(1);
    }

    pub fn observe_rtt(&mut self, micros: u32) {
        self.rtt.observe(micros);
    }

    pub fn record_park(&mut self, at_micros: u64, parked: bool) {
        self.parks.push(ParkEvent { at_micros, parked });
    }

    pub const fn close(&mut self, at_micros: u64) {
        self.closed_at_micros = Some(at_micros);
    }

    /// Bytes of the payload this lane moved, from this end's point of view.
    ///
    /// A sender never receives chunks and a receiver never sends them, so a figure fixed to one
    /// direction reads as zero on the other end — and a report of zeros is worse than no report,
    /// because it looks like a medium that carried nothing.
    pub const fn carried_bytes(&self, role: TransferRole) -> u64 {
        match role {
            TransferRole::Sending => self.sent_bytes,
            TransferRole::Receiving => self.received_bytes,
        }
    }

    pub const fn carried_chunks(&self, role: TransferRole) -> u64 {
        match role {
            TransferRole::Sending => self.sent_chunks,
            TransferRole::Receiving => self.received_chunks,
        }
    }

    /// Closes the current window and opens the next.
    pub fn sample(&mut self, at_micros: u64, role: TransferRole) -> ThroughputSample {
        let cumulative = self.carried_bytes(role);
        let elapsed = at_micros.saturating_sub(self.window_started_micros);
        let carried = cumulative.saturating_sub(self.window_start_bytes);
        let kbps = rate_kbps(carried, elapsed);
        self.peak_kbps = self.peak_kbps.max(kbps);
        self.window_started_micros = at_micros;
        self.window_start_bytes = cumulative;
        ThroughputSample {
            at_micros,
            lane: self.lane,
            medium: self.medium,
            cumulative_bytes: cumulative,
            kbps,
        }
    }

    /// Average over the whole time this lane was open.
    pub fn mean_kbps(&self, now_micros: u64, role: TransferRole) -> u32 {
        let until = self.closed_at_micros.unwrap_or(now_micros);
        rate_kbps(
            self.carried_bytes(role),
            until.saturating_sub(self.opened_at_micros),
        )
    }

    pub const fn open_micros(&self, now_micros: u64) -> u64 {
        let until = match self.closed_at_micros {
            Some(at) => at,
            None => now_micros,
        };
        until.saturating_sub(self.opened_at_micros)
    }
}

/// Kilobits per second from bytes and microseconds, without leaving integers.
pub fn rate_kbps(bytes: u64, elapsed_micros: u64) -> u32 {
    if elapsed_micros == 0 {
        return 0;
    }
    let kbps = bytes.saturating_mul(8_000) / elapsed_micros;
    u32::try_from(kbps).unwrap_or(u32::MAX)
}
