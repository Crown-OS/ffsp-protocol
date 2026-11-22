use std::collections::{BTreeMap, VecDeque};

use crate::medium::LaneProfile;
use crate::transfer::{
    CHUNK_LEN, ChunkBitmap, ChunkLayout, LaneEstimator, LaneLoad, earliest_completion,
};
use crate::wire::{ChunkIndex, LaneId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assignment {
    pub index: ChunkIndex,
    pub lane: LaneId,
    pub len: u32,
    /// A second copy of a tail chunk, sent so one slow lane cannot decide when the transfer ends.
    pub duplicate: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub completed_chunks: u32,
    pub total_chunks: u32,
    pub delivered_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct InFlight {
    sent_at_millis: u64,
    deadline_millis: u64,
    len: u32,
    /// What the lane had to send before this chunk, which a stall is charged for too.
    queued_ahead_bytes: u64,
}

#[derive(Debug)]
struct LaneState {
    estimator: LaneEstimator,
    queued_bytes: u64,
    inflight_chunks: u32,
    /// When this lane last finished something, so the next chunk is charged only for the time it
    /// spent being sent rather than the time it spent queued behind its predecessors.
    idle_since_millis: u64,
    delivered_bytes: u64,
    delivered_chunks: u32,
    duplicate_wins: u32,
    stalls: u32,
    suspended: bool,
    parked: bool,
    /// Whether any acknowledgement has measured this lane yet, rather than its profile's hint.
    measured: bool,
}

impl LaneState {
    const fn new(profile: &LaneProfile) -> Self {
        Self {
            estimator: LaneEstimator::from_profile(profile),
            queued_bytes: 0,
            inflight_chunks: 0,
            idle_since_millis: 0,
            delivered_bytes: 0,
            delivered_chunks: 0,
            duplicate_wins: 0,
            stalls: 0,
            suspended: false,
            parked: false,
            measured: false,
        }
    }

    const fn is_working(&self) -> bool {
        !self.suspended && !self.parked
    }

    /// Charges a lane for the time it actually spent sending a chunk.
    ///
    /// A lane sends its queue one chunk at a time, so a chunk dispatched while three others were
    /// outstanding did not take from its dispatch to its arrival — it took from whenever the lane
    /// got to it. Charging the whole wait would divide every lane's measured bandwidth by its
    /// queue depth, and the resulting figure is what the report would print.
    fn observe_service(&mut self, sent_at_millis: u64, now_millis: u64, len: u32) {
        let began = sent_at_millis.max(self.idle_since_millis);
        let elapsed_micros = now_millis.saturating_sub(began).saturating_mul(1_000);
        if self.measured {
            self.estimator
                .observe_delivery(u64::from(len), elapsed_micros);
        } else {
            self.estimator
                .observe_first_delivery(u64::from(len), elapsed_micros);
        }
        self.idle_since_millis = now_millis;
        self.measured = true;
    }

    /// How many chunks this lane may hold at once, from what it is measured to carry.
    ///
    /// A uniform depth lets the slowest lane queue as much work as the fastest, and since queued
    /// work is what the tail waits on, four chunks on a Bluetooth link is twenty seconds of
    /// nothing at the end of a transfer. One chunk per unit of capacity keeps every lane's queue
    /// about the same length in time rather than in chunks. A hint earns nothing: a lane holds one
    /// chunk until an acknowledgement has measured it, since queueing on a wrong hint is what
    /// stalls.
    const fn inflight_limit(&self, ceiling: u32) -> u32 {
        if !self.measured {
            return 1;
        }
        let earned = self
            .estimator
            .bandwidth_kbps()
            .div_ceil(ChunkScheduler::KBPS_PER_INFLIGHT_CHUNK);
        if earned < 1 {
            1
        } else if earned > ceiling {
            ceiling
        } else {
            earned
        }
    }
}

/// What one lane actually did, as opposed to what it was predicted to do.
///
/// The scheduler knows this per lane already; without it a report can say a transfer used three
/// mediums but not what any of them carried, which is the only question worth asking of striping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneStats {
    pub lane: LaneId,
    pub delivered_bytes: u64,
    pub delivered_chunks: u32,
    /// Tail copies this lane won, having been sent a chunk another lane was still carrying.
    pub duplicate_wins: u32,
    pub stalls: u32,
    pub inflight_chunks: u32,
    pub inflight_limit: u32,
    pub bandwidth_kbps: u32,
    pub rtt_micros: u32,
    pub suspended: bool,
    pub parked: bool,
}

/// Decides which chunk goes down which lane.
///
/// Order is irrelevant here, which is what makes the whole approach work: because a chunk is
/// verifiable on its own, a chunk that stalls can simply be handed to another lane, and a chunk
/// that then arrives twice costs nothing. There is no reorder buffer, no retransmit timer and no
/// head-of-line blocking between lanes.
#[derive(Debug)]
pub struct ChunkScheduler {
    layout: ChunkLayout,
    bitmap: ChunkBitmap,
    lanes: BTreeMap<LaneId, LaneState>,
    inflight: BTreeMap<(ChunkIndex, LaneId), InFlight>,
    /// Stalled chunks whose lane may still deliver them. Their late acknowledgement is the only
    /// measurement that can restore an estimate the stall lowered.
    overdue: BTreeMap<(ChunkIndex, LaneId), InFlight>,
    pending: VecDeque<ChunkIndex>,
    next_unsent: u32,
    delivered_bytes: u64,
    max_inflight_per_lane: u32,
}

impl ChunkScheduler {
    /// Enough outstanding chunks to keep a fast lane saturated without letting one lane hoard the
    /// buffer pool.
    pub const DEFAULT_MAX_INFLIGHT_PER_LANE: u32 = 4;
    /// Capacity that earns a lane one more chunk in flight.
    const KBPS_PER_INFLIGHT_CHUNK: u32 = 64_000;
    /// A chunk is considered stalled once sending it takes this many times its estimate.
    const STALL_MULTIPLIER: u64 = 3;
    /// Floor on the stall deadline, so an unmeasured hint cannot reassign a healthy lane's work.
    /// There is deliberately no ceiling: one megabyte on Bluetooth takes five seconds, and a cap
    /// below that stalls every chunk the lane is given.
    const MIN_STALL_MILLIS: u64 = 1_000;

    pub fn new(layout: ChunkLayout) -> Self {
        let bitmap = ChunkBitmap::new(layout.count());
        Self::resuming(layout, bitmap)
    }

    /// Continues a transfer from a persisted bitmap.
    pub fn resuming(layout: ChunkLayout, bitmap: ChunkBitmap) -> Self {
        let delivered_bytes = u64::from(bitmap.completed()) * u64::from(CHUNK_LEN);
        Self {
            layout,
            bitmap,
            lanes: BTreeMap::new(),
            inflight: BTreeMap::new(),
            overdue: BTreeMap::new(),
            pending: VecDeque::new(),
            next_unsent: 0,
            delivered_bytes: delivered_bytes.min(layout.total_len()),
            max_inflight_per_lane: Self::DEFAULT_MAX_INFLIGHT_PER_LANE,
        }
    }

    pub fn add_lane(&mut self, lane: LaneId, profile: &LaneProfile) {
        self.lanes.insert(lane, LaneState::new(profile));
    }

    /// Raises or lowers the ceiling on how many chunks any one lane may hold at once.
    ///
    /// [`Tuning`](crate::engine::Tuning) carries this figure; before this it had no way in.
    pub fn set_max_inflight_per_lane(&mut self, limit: u32) {
        self.max_inflight_per_lane = limit.max(1);
    }

    /// Takes back a chunk that never reached the wire.
    ///
    /// A lane's writer drops a chunk another lane already delivered. Without this the scheduler
    /// would hold it as in flight until the stall timer expired, which on a slow lane is seconds
    /// of the tail spent waiting for something that was never sent.
    pub fn abandon(&mut self, index: ChunkIndex, lane: LaneId) -> bool {
        self.overdue.remove(&(index, lane));
        let Some(flight) = self.inflight.remove(&(index, lane)) else {
            return false;
        };
        if let Some(state) = self.lanes.get_mut(&lane) {
            state.queued_bytes = state.queued_bytes.saturating_sub(u64::from(flight.len));
            state.inflight_chunks = state.inflight_chunks.saturating_sub(1);
        }
        !self.bitmap.contains(index) && self.requeue(index)
    }

    /// Records a round trip measured on the lane itself.
    ///
    /// Chunk timings cannot stand in for this: a megabyte on Bluetooth measures throughput, and
    /// feeding that in as latency would have `completion_micros` charge the lane for its own
    /// serialisation delay a second time.
    pub fn observe_rtt(&mut self, lane: LaneId, micros: u32) {
        if let Some(state) = self.lanes.get_mut(&lane) {
            state.estimator.observe_rtt(micros);
        }
    }

    /// What each lane has carried so far.
    pub fn lane_stats(&self) -> impl Iterator<Item = LaneStats> + '_ {
        let ceiling = self.max_inflight_per_lane;
        self.lanes.iter().map(move |(lane, state)| LaneStats {
            lane: *lane,
            delivered_bytes: state.delivered_bytes,
            delivered_chunks: state.delivered_chunks,
            duplicate_wins: state.duplicate_wins,
            stalls: state.stalls,
            inflight_chunks: state.inflight_chunks,
            inflight_limit: state.inflight_limit(ceiling),
            bandwidth_kbps: state.estimator.bandwidth_kbps(),
            rtt_micros: state.estimator.rtt_micros(),
            suspended: state.suspended,
            parked: state.parked,
        })
    }

    /// Drops a lane and returns its outstanding chunks to the queue.
    pub fn remove_lane(&mut self, lane: LaneId) -> usize {
        self.lanes.remove(&lane);
        self.overdue.retain(|(_, owner), _| *owner != lane);
        self.reclaim(lane).len()
    }

    /// Takes back everything a lane is holding, so another lane can carry it.
    fn reclaim(&mut self, lane: LaneId) -> Vec<ChunkIndex> {
        let orphaned: Vec<ChunkIndex> = self
            .inflight
            .keys()
            .filter(|(_, owner)| *owner == lane)
            .map(|(index, _)| *index)
            .collect();
        let mut handed_back = Vec::with_capacity(orphaned.len());
        for index in orphaned {
            if let Some(flight) = self.inflight.remove(&(index, lane))
                && let Some(state) = self.lanes.get_mut(&lane)
            {
                state.queued_bytes = state.queued_bytes.saturating_sub(u64::from(flight.len));
                state.inflight_chunks = state.inflight_chunks.saturating_sub(1);
            }
            if !self.bitmap.contains(index) {
                self.requeue(index);
            }
            handed_back.push(index);
        }
        handed_back
    }

    /// Suspends or resumes a lane, returning any chunks a suspension handed back.
    ///
    /// Leaving them in flight would mean a suspended lane kept delivering throughout the window
    /// meant to measure the transfer without it, which is the one thing the probe must not
    /// measure. The engine abandons the returned chunks on that lane's writer.
    pub fn set_suspended(&mut self, lane: LaneId, suspended: bool) -> Vec<ChunkIndex> {
        let Some(state) = self.lanes.get_mut(&lane) else {
            return Vec::new();
        };
        state.suspended = suspended;
        if !suspended {
            return Vec::new();
        }
        self.reclaim(lane)
    }

    pub fn set_parked(&mut self, lane: LaneId, parked: bool) {
        if let Some(state) = self.lanes.get_mut(&lane) {
            state.parked = parked;
        }
    }

    pub fn next_assignment(&mut self, now_millis: u64) -> Option<Assignment> {
        if self.bitmap.is_complete() {
            return None;
        }
        let loads = self.working_loads();
        if loads.is_empty() {
            return None;
        }
        let unsent = self.layout.count().saturating_sub(self.next_unsent);
        let backlog_chunks = u32::try_from(self.pending.len())
            .unwrap_or(u32::MAX)
            .saturating_add(unsent);

        while let Some(index) = self.pending.pop_front() {
            if self.bitmap.contains(index) {
                continue;
            }
            let Some(assignment) = self.dispatch(index, &loads, now_millis, backlog_chunks) else {
                self.pending.push_front(index);
                return None;
            };
            return Some(assignment);
        }
        while self.next_unsent < self.layout.count() {
            let index = ChunkIndex::new(self.next_unsent);
            if self.bitmap.contains(index) {
                self.next_unsent += 1;
                continue;
            }
            let assignment = self.dispatch(index, &loads, now_millis, backlog_chunks)?;
            self.next_unsent += 1;
            return Some(assignment);
        }
        self.duplicate_tail(&loads, now_millis)
    }

    /// Records a chunk as arrived, and retires any redundant copy still in flight elsewhere.
    pub fn on_delivered(&mut self, index: ChunkIndex, lane: LaneId, now_millis: u64) -> bool {
        if let Some(flight) = self.inflight.remove(&(index, lane))
            && let Some(state) = self.lanes.get_mut(&lane)
        {
            state.queued_bytes = state.queued_bytes.saturating_sub(u64::from(flight.len));
            state.inflight_chunks = state.inflight_chunks.saturating_sub(1);
            state.observe_service(flight.sent_at_millis, now_millis, flight.len);
        } else if let Some(flight) = self.overdue.remove(&(index, lane))
            && let Some(state) = self.lanes.get_mut(&lane)
        {
            state.observe_service(flight.sent_at_millis, now_millis, flight.len);
        }
        let retired = self.retire_duplicates_of(index);

        let len = self.layout.len_at(index).unwrap_or_default();
        if self.bitmap.insert(index) {
            self.delivered_bytes = self.delivered_bytes.saturating_add(u64::from(len));
            if let Some(state) = self.lanes.get_mut(&lane) {
                state.delivered_bytes = state.delivered_bytes.saturating_add(u64::from(len));
                state.delivered_chunks = state.delivered_chunks.saturating_add(1);
                if retired > 0 {
                    state.duplicate_wins = state.duplicate_wins.saturating_add(1);
                }
            }
            true
        } else {
            false
        }
    }

    /// Returns chunks whose lane has gone quiet for long enough to hand them elsewhere, with the
    /// lane that was holding each.
    ///
    /// The stalling lane's estimate is capped at what it provably did not beat, which is what
    /// stops earliest-completion from handing the same chunk straight back to it. The chunk stays
    /// overdue on that lane, so a late acknowledgement still corrects the estimate.
    pub fn detect_stalls(&mut self, now_millis: u64) -> Vec<(ChunkIndex, LaneId)> {
        let stalled: Vec<(ChunkIndex, LaneId)> = self
            .inflight
            .iter()
            .filter(|(_, flight)| now_millis >= flight.deadline_millis)
            .map(|(key, _)| *key)
            .collect();

        for (index, lane) in &stalled {
            if let Some(flight) = self.inflight.remove(&(*index, *lane)) {
                if let Some(state) = self.lanes.get_mut(lane) {
                    state.queued_bytes = state.queued_bytes.saturating_sub(u64::from(flight.len));
                    state.inflight_chunks = state.inflight_chunks.saturating_sub(1);
                    state.stalls = state.stalls.saturating_add(1);
                    let elapsed_micros = now_millis
                        .saturating_sub(flight.sent_at_millis)
                        .saturating_mul(1_000);
                    let owed = flight.queued_ahead_bytes + u64::from(flight.len);
                    state.estimator.cap_delivery(owed, elapsed_micros);
                }
                self.overdue.insert((*index, *lane), flight);
            }
            if !self.bitmap.contains(*index) {
                self.requeue(*index);
            }
        }
        stalled
    }

    pub const fn is_complete(&self) -> bool {
        self.bitmap.is_complete()
    }

    pub const fn delivered_bytes(&self) -> u64 {
        self.delivered_bytes
    }

    pub const fn bitmap(&self) -> &ChunkBitmap {
        &self.bitmap
    }

    pub const fn progress(&self) -> Progress {
        Progress {
            completed_chunks: self.bitmap.completed(),
            total_chunks: self.bitmap.count(),
            delivered_bytes: self.delivered_bytes,
            total_bytes: self.layout.total_len(),
        }
    }

    pub fn working_lane_count(&self) -> usize {
        self.lanes
            .values()
            .filter(|state| state.is_working())
            .count()
    }

    /// The lane the probe should try losing: the slowest one still working.
    pub fn weakest_working_lane(&self) -> Option<LaneId> {
        if self.working_lane_count() < 2 {
            return None;
        }
        self.lanes
            .iter()
            .filter(|(_, state)| state.is_working())
            .min_by_key(|(lane, state)| (state.estimator.bandwidth_kbps(), **lane))
            .map(|(lane, _)| *lane)
    }

    fn requeue(&mut self, index: ChunkIndex) -> bool {
        if self.pending.contains(&index) {
            return false;
        }
        self.pending.push_back(index);
        true
    }

    /// Cancels copies of a chunk still outstanding elsewhere, returning how many there were.
    fn retire_duplicates_of(&mut self, index: ChunkIndex) -> usize {
        let duplicates: Vec<(ChunkIndex, LaneId)> = self
            .inflight
            .range((index, LaneId::HANDSHAKE)..=(index, LaneId::MAX))
            .map(|(key, _)| *key)
            .collect();
        let retired = duplicates.len();
        for key in duplicates {
            if let Some(flight) = self.inflight.remove(&key)
                && let Some(state) = self.lanes.get_mut(&key.1)
            {
                state.queued_bytes = state.queued_bytes.saturating_sub(u64::from(flight.len));
                state.inflight_chunks = state.inflight_chunks.saturating_sub(1);
            }
        }
        retired
    }

    fn working_loads(&self) -> Vec<LaneLoad> {
        self.lanes
            .iter()
            .filter(|(_, state)| {
                state.is_working()
                    && state.inflight_chunks < state.inflight_limit(self.max_inflight_per_lane)
            })
            .map(|(lane, state)| LaneLoad {
                lane: *lane,
                queued_bytes: state.queued_bytes,
                estimator: state.estimator,
            })
            .collect()
    }

    /// The working lane with the most measured capacity, whether or not it has room right now.
    fn fastest_working_lane(&self) -> Option<LaneLoad> {
        self.lanes
            .iter()
            .filter(|(_, state)| state.is_working())
            .max_by_key(|(lane, state)| {
                (state.estimator.bandwidth_kbps(), std::cmp::Reverse(**lane))
            })
            .map(|(lane, state)| LaneLoad {
                lane: *lane,
                queued_bytes: state.queued_bytes,
                estimator: state.estimator,
            })
    }

    /// Earliest completion first, among the lanes that would not hold up the end of the transfer.
    ///
    /// A lane may take a chunk only if it would deliver it before the fastest lane could have
    /// sent everything still unassigned, this chunk included. Otherwise waiting for the fastest
    /// lane to free up is quicker, and handing the chunk out is what leaves a whole transfer
    /// waiting on the last megabyte crossing Bluetooth.
    fn dispatch(
        &mut self,
        index: ChunkIndex,
        loads: &[LaneLoad],
        now_millis: u64,
        backlog_chunks: u32,
    ) -> Option<Assignment> {
        let len = self.layout.len_at(index)?;
        let fastest = self.fastest_working_lane()?;
        let backlog_bytes = u64::from(backlog_chunks.max(1)) * u64::from(CHUNK_LEN);
        let fastest_finish = LaneLoad {
            queued_bytes: fastest.queued_bytes.saturating_add(backlog_bytes),
            ..fastest
        }
        .completion_micros(0);
        let eligible: Vec<LaneLoad> = loads
            .iter()
            .filter(|load| {
                load.lane == fastest.lane || load.completion_micros(len) <= fastest_finish
            })
            .copied()
            .collect();
        let lane = earliest_completion(&eligible, len)?;
        Some(self.assign(index, lane, len, now_millis, false))
    }

    fn assign(
        &mut self,
        index: ChunkIndex,
        lane: LaneId,
        len: u32,
        now_millis: u64,
        duplicate: bool,
    ) -> Assignment {
        let mut queued_ahead_bytes = 0;
        let mut deadline_millis = now_millis;
        if let Some(state) = self.lanes.get_mut(&lane) {
            queued_ahead_bytes = state.queued_bytes;
            state.queued_bytes = state.queued_bytes.saturating_add(u64::from(len));
            state.inflight_chunks += 1;
            let ahead_millis = state.estimator.drain_micros(queued_ahead_bytes) / 1_000;
            let sending_millis = state.estimator.drain_micros(u64::from(len)) / 1_000;
            deadline_millis += ahead_millis
                + (sending_millis * Self::STALL_MULTIPLIER).max(Self::MIN_STALL_MILLIS);
        }
        self.inflight.insert(
            (index, lane),
            InFlight {
                sent_at_millis: now_millis,
                deadline_millis,
                len,
                queued_ahead_bytes,
            },
        );
        Assignment {
            index,
            lane,
            len,
            duplicate,
        }
    }

    /// Sends an outstanding chunk a second time once the tail is all that remains, to a lane that
    /// would deliver it before the lane already holding it.
    fn duplicate_tail(&mut self, loads: &[LaneLoad], now_millis: u64) -> Option<Assignment> {
        let lane_count = u32::try_from(self.working_lane_count()).unwrap_or(u32::MAX);
        if self.bitmap.remaining() > lane_count.saturating_mul(2) {
            return None;
        }
        let mut outstanding: Vec<(u64, ChunkIndex, LaneId)> = self
            .inflight
            .iter()
            .map(|((index, lane), flight)| (flight.sent_at_millis, *index, *lane))
            .collect();
        outstanding.sort_unstable();
        outstanding.into_iter().find_map(|(_, index, holder)| {
            let len = self.layout.len_at(index)?;
            let holder_finish = self.lanes.get(&holder).map(|state| {
                LaneLoad {
                    lane: holder,
                    queued_bytes: state.queued_bytes,
                    estimator: state.estimator,
                }
                .completion_micros(0)
            })?;
            let eligible: Vec<LaneLoad> = loads
                .iter()
                .filter(|load| {
                    !self.inflight.contains_key(&(index, load.lane))
                        && load.completion_micros(len) < holder_finish
                })
                .copied()
                .collect();
            let lane = earliest_completion(&eligible, len)?;
            Some(self.assign(index, lane, len, now_millis, true))
        })
    }
}
