#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use proptest::prelude::*;

use super::*;
use crate::medium::LaneProfile;
use crate::wire::{ChunkIndex, LaneId};

/// Deterministic filler so a failure is reproducible without a random seed.
fn payload_of(len: usize) -> Vec<u8> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 33) as u8
        })
        .collect()
}

fn verifier_for(payload: &[u8]) -> (ChunkVerifier, ChunkLayout) {
    let layout = ChunkLayout::new(payload.len() as u64).unwrap();
    let values = chaining_values_of(payload, &layout).unwrap();
    let root = blake3::hash(payload);
    (ChunkVerifier::new(root, layout, values).unwrap(), layout)
}

const MIB: usize = 1 << 20;

fn interesting_lengths() -> Vec<usize> {
    vec![
        1,
        1023,
        1024,
        1025,
        MIB - 1,
        MIB,
        MIB + 1,
        2 * MIB - 1,
        2 * MIB,
        2 * MIB + 1,
        3 * MIB,
        3 * MIB + 12345,
        4 * MIB,
        5 * MIB - 7,
        5 * MIB,
    ]
}

#[test]
fn folded_chaining_values_equal_the_blake3_root() {
    for len in interesting_lengths() {
        let payload = payload_of(len);
        let layout = ChunkLayout::new(len as u64).unwrap();
        let values = chaining_values_of(&payload, &layout).unwrap();
        if layout.is_single_subtree() {
            continue;
        }
        assert_eq!(
            fold_root(&values, &layout).unwrap(),
            blake3::hash(&payload),
            "root mismatch at {len} bytes"
        );
    }
}

#[test]
fn every_chunk_verifies_independently_and_out_of_order() {
    let payload = payload_of(5 * MIB - 7);
    let (verifier, layout) = verifier_for(&payload);

    let mut indices: Vec<ChunkIndex> = layout.indices().collect();
    indices.reverse();
    for index in indices {
        let offset = layout.byte_offset(index).unwrap() as usize;
        let len = layout.len_at(index).unwrap() as usize;
        verifier
            .verify(index, &payload[offset..offset + len])
            .unwrap();
    }
}

#[test]
fn a_flipped_bit_is_caught() {
    let payload = payload_of(3 * MIB);
    let (verifier, layout) = verifier_for(&payload);

    let index = ChunkIndex::new(1);
    let offset = layout.byte_offset(index).unwrap() as usize;
    let len = layout.len_at(index).unwrap() as usize;
    let mut chunk = payload[offset..offset + len].to_vec();
    chunk[999] ^= 0x01;

    assert_eq!(
        verifier.verify(index, &chunk),
        Err(TransferError::ChunkCorrupt { index })
    );
}

#[test]
fn a_chunk_replayed_at_another_position_is_caught() {
    let payload = payload_of(4 * MIB);
    let (verifier, layout) = verifier_for(&payload);

    let source = ChunkIndex::new(0);
    let offset = layout.byte_offset(source).unwrap() as usize;
    let len = layout.len_at(source).unwrap() as usize;
    let chunk = &payload[offset..offset + len];

    verifier.verify(source, chunk).unwrap();
    let elsewhere = ChunkIndex::new(2);
    assert_eq!(
        verifier.verify(elsewhere, chunk),
        Err(TransferError::ChunkCorrupt { index: elsewhere })
    );
}

#[test]
fn a_tampered_chaining_value_list_is_rejected_at_construction() {
    let payload = payload_of(3 * MIB);
    let layout = ChunkLayout::new(payload.len() as u64).unwrap();
    let mut values = chaining_values_of(&payload, &layout).unwrap();
    values[2][0] ^= 0xFF;

    assert_eq!(
        ChunkVerifier::new(blake3::hash(&payload), layout, values).map(|_| ()),
        Err(TransferError::RootMismatch)
    );
}

#[test]
fn a_short_chaining_value_list_is_rejected() {
    let payload = payload_of(3 * MIB);
    let layout = ChunkLayout::new(payload.len() as u64).unwrap();
    let mut values = chaining_values_of(&payload, &layout).unwrap();
    values.pop();

    assert_eq!(
        ChunkVerifier::new(blake3::hash(&payload), layout, values).map(|_| ()),
        Err(TransferError::ChainingValueCountMismatch {
            expected: 3,
            actual: 2
        })
    );
}

#[test]
fn a_single_chunk_payload_verifies_against_the_root_alone() {
    let payload = payload_of(4096);
    let (verifier, _) = verifier_for(&payload);
    verifier.verify(ChunkIndex::ZERO, &payload).unwrap();

    let mut tampered = payload;
    tampered[0] ^= 0x01;
    assert_eq!(
        verifier.verify(ChunkIndex::ZERO, &tampered),
        Err(TransferError::ChunkCorrupt {
            index: ChunkIndex::ZERO
        })
    );
}

#[test]
fn a_truncated_chunk_is_caught_before_hashing() {
    let payload = payload_of(2 * MIB);
    let (verifier, _) = verifier_for(&payload);
    let index = ChunkIndex::ZERO;
    assert_eq!(
        verifier.verify(index, &payload[..MIB - 1]),
        Err(TransferError::ChunkLengthMismatch {
            index,
            expected: CHUNK_LEN,
            actual: MIB - 1
        })
    );
}

#[test]
fn an_index_past_the_end_is_rejected() {
    let payload = payload_of(MIB);
    let (verifier, _) = verifier_for(&payload);
    let index = ChunkIndex::new(1);
    assert_eq!(
        verifier.verify(index, &payload),
        Err(TransferError::ChunkOutOfRange { index, count: 1 })
    );
}

#[test]
fn layout_covers_the_payload_exactly() {
    for len in interesting_lengths() {
        let layout = ChunkLayout::new(len as u64).unwrap();
        let covered: u64 = layout
            .indices()
            .map(|i| u64::from(layout.len_at(i).unwrap()))
            .sum();
        assert_eq!(covered, len as u64, "layout does not cover {len} bytes");
        assert!(layout.len_at(ChunkIndex::new(layout.count())).is_none());
    }
}

#[test]
fn an_empty_payload_has_no_chunks() {
    let layout = ChunkLayout::new(0).unwrap();
    assert_eq!(layout.count(), 0);
    assert_eq!(layout.indices().count(), 0);
    assert!(ChunkVerifier::new(blake3::hash(b""), layout, Vec::new()).is_ok());
}

#[test]
fn a_payload_needing_more_than_u32_chunks_is_rejected() {
    let too_large = (u64::from(u32::MAX) + 1) * u64::from(CHUNK_LEN);
    assert_eq!(
        ChunkLayout::new(too_large),
        Err(TransferError::PayloadTooLarge(too_large))
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn folding_matches_blake3_at_any_length(len in (MIB + 1)..(6 * MIB)) {
        let payload = payload_of(len);
        let layout = ChunkLayout::new(len as u64).unwrap();
        let values = chaining_values_of(&payload, &layout).unwrap();
        prop_assert_eq!(fold_root(&values, &layout).unwrap(), blake3::hash(&payload));
    }
}

// ---------------------------------------------------------------- manifest

#[test]
fn a_manifest_entry_cannot_escape_the_destination() {
    let root = blake3::hash(b"");
    for hostile in [
        "/etc/passwd",
        "../../etc/passwd",
        "nested/../../escape",
        "..",
        "",
        "C:\\Windows\\System32",
        "\\\\server\\share",
        "with\0null",
        "./relative",
    ] {
        assert!(
            FileEntry::new(hostile.to_owned(), 0, root).is_err(),
            "accepted hostile name {hostile:?}"
        );
    }
}

#[test]
fn ordinary_relative_names_are_accepted() {
    let root = blake3::hash(b"");
    for benign in [
        "photo.jpg",
        "holiday/photo.jpg",
        "a/b/c/d.bin",
        "..dotfile",
        "file..txt",
    ] {
        assert!(
            FileEntry::new(benign.to_owned(), 1, root).is_ok(),
            "rejected {benign:?}"
        );
    }
}

#[test]
fn a_manifest_totals_its_entries() {
    let root = blake3::hash(b"");
    let manifest = Manifest::new(vec![
        FileEntry::new("a".to_owned(), 100, root).unwrap(),
        FileEntry::new("b".to_owned(), 200, root).unwrap(),
    ]);
    assert_eq!(manifest.total_len(), 300);
    assert_eq!(manifest.len(), 2);
}

// ---------------------------------------------------------------- bitmap

#[test]
fn a_bitmap_tracks_completion_and_ignores_repeats() {
    let mut bitmap = ChunkBitmap::new(200);
    assert!(bitmap.insert(ChunkIndex::new(7)));
    assert!(!bitmap.insert(ChunkIndex::new(7)));
    assert_eq!(bitmap.completed(), 1);
    assert_eq!(bitmap.remaining(), 199);
    assert!(bitmap.contains(ChunkIndex::new(7)));
    assert!(!bitmap.contains(ChunkIndex::new(8)));
    assert!(!bitmap.insert(ChunkIndex::new(200)));
}

#[test]
fn a_bitmap_for_ten_gigabytes_costs_under_two_kilobytes() {
    let layout = ChunkLayout::new(10 * 1024 * MIB as u64).unwrap();
    let bitmap = ChunkBitmap::new(layout.count());
    assert_eq!(layout.count(), 10_240);
    assert_eq!(bitmap.to_bytes().len(), 1_280);
}

#[test]
fn a_bitmap_round_trips_through_bytes() {
    let mut bitmap = ChunkBitmap::new(130);
    for index in [0, 1, 63, 64, 65, 129] {
        bitmap.insert(ChunkIndex::new(index));
    }
    let restored = ChunkBitmap::from_bytes(&bitmap.to_bytes(), 130).unwrap();
    assert_eq!(restored, bitmap);
    assert_eq!(restored.completed(), 6);
    assert_eq!(restored.missing().count(), 124);
}

#[test]
fn a_bitmap_of_the_wrong_size_is_rejected() {
    assert_eq!(
        ChunkBitmap::from_bytes(&[0; 7], 130),
        Err(TransferError::BitmapLength {
            expected: 24,
            actual: 7
        })
    );
}

#[test]
fn resume_state_round_trips() {
    let mut bitmap = ChunkBitmap::new(64);
    bitmap.insert(ChunkIndex::new(3));
    let state = ResumeState::new(blake3::hash(b"payload"), 64 * MIB as u64, bitmap);
    let restored = ResumeState::parse(&state.to_bytes()).unwrap();
    assert_eq!(restored, state);
    assert_eq!(restored.bitmap().completed(), 1);
}

// ---------------------------------------------------------------- assignment

fn load(lane: u16, kbps: u32, queued: u64) -> LaneLoad {
    let mut profile = LaneProfile::LOOPBACK;
    profile.bandwidth_hint_kbps = kbps;
    profile.rtt_hint_micros = 0;
    LaneLoad {
        lane: LaneId::new(lane),
        queued_bytes: queued,
        estimator: LaneEstimator::from_profile(&profile),
    }
}

#[test]
fn earliest_completion_prefers_the_lane_that_finishes_first_not_the_fastest() {
    let fast_but_backed_up = load(1, 400_000, 64 * MIB as u64);
    let slow_but_idle = load(2, 40_000, 0);
    assert_eq!(
        earliest_completion(&[fast_but_backed_up, slow_but_idle], CHUNK_LEN),
        Some(LaneId::new(2))
    );
}

#[test]
fn earliest_completion_prefers_the_faster_lane_when_queues_match() {
    assert_eq!(
        earliest_completion(&[load(1, 40_000, 0), load(2, 400_000, 0)], CHUNK_LEN),
        Some(LaneId::new(2))
    );
}

#[test]
fn earliest_completion_of_nothing_is_nothing() {
    assert_eq!(earliest_completion(&[], CHUNK_LEN), None);
}

#[test]
fn an_estimator_moves_toward_a_sustained_measurement() {
    let mut estimator = LaneEstimator::from_profile(&LaneProfile::WIFI_INFRA);
    let optimistic = estimator.bandwidth_kbps();
    // One chunk per second is 8388 kbps; the estimate should home in on it.
    let truth = u64::from(CHUNK_LEN) * 8_000 / 1_000_000;

    for _ in 0..20 {
        estimator.observe_delivery(u64::from(CHUNK_LEN), 1_000_000);
    }
    let after_twenty = u64::from(estimator.bandwidth_kbps());
    assert!(after_twenty < u64::from(optimistic) / 10);
    assert!(after_twenty > truth, "overshot to {after_twenty}");

    for _ in 0..20 {
        estimator.observe_delivery(u64::from(CHUNK_LEN), 1_000_000);
    }
    let after_forty = u64::from(estimator.bandwidth_kbps());
    assert!(after_forty < after_twenty);
    assert!(
        after_forty.abs_diff(truth) * 100 / truth < 1,
        "{after_forty} is not within one percent of {truth}"
    );
}

#[test]
fn an_estimator_ignores_a_zero_length_interval() {
    let mut estimator = LaneEstimator::from_profile(&LaneProfile::WIFI_INFRA);
    let before = estimator.bandwidth_kbps();
    estimator.observe_delivery(1_000, 0);
    assert_eq!(estimator.bandwidth_kbps(), before);
}

// ---------------------------------------------------------------- scheduler simulation

fn profile_with(kbps: u32, rtt_micros: u32) -> LaneProfile {
    LaneProfile {
        bandwidth_hint_kbps: kbps,
        rtt_hint_micros: rtt_micros,
        ..LaneProfile::LOOPBACK
    }
}

fn transmit_millis(len: u32, kbps: u32) -> u64 {
    (u64::from(len) * 8_000 / u64::from(kbps.max(1)) / 1_000).max(1)
}

#[derive(Debug)]
struct SimulatedLane {
    kbps: u32,
    free_at_millis: u64,
    inflight: Vec<(ChunkIndex, u64)>,
    delivered: u32,
}

#[derive(Debug)]
struct Simulation {
    scheduler: ChunkScheduler,
    lanes: std::collections::BTreeMap<LaneId, SimulatedLane>,
    now_millis: u64,
}

impl Simulation {
    fn new(total_len: u64, specs: &[(u32, u32)]) -> Self {
        let layout = ChunkLayout::new(total_len).unwrap();
        let mut scheduler = ChunkScheduler::new(layout);
        let mut lanes = std::collections::BTreeMap::new();
        for (index, (kbps, rtt)) in specs.iter().enumerate() {
            let lane = LaneId::new(u16::try_from(index + 1).unwrap());
            scheduler.add_lane(lane, &profile_with(*kbps, *rtt));
            lanes.insert(
                lane,
                SimulatedLane {
                    kbps: *kbps,
                    free_at_millis: 0,
                    inflight: Vec::new(),
                    delivered: 0,
                },
            );
        }
        Self {
            scheduler,
            lanes,
            now_millis: 0,
        }
    }

    fn lane(&self, index: usize) -> LaneId {
        LaneId::new(u16::try_from(index + 1).unwrap())
    }

    /// Hands out every assignment the scheduler is willing to make right now.
    fn dispatch_all(&mut self) {
        while let Some(assignment) = self.scheduler.next_assignment(self.now_millis) {
            let Some(lane) = self.lanes.get_mut(&assignment.lane) else {
                continue;
            };
            let starts = lane.free_at_millis.max(self.now_millis);
            let arrives = starts + transmit_millis(assignment.len, lane.kbps);
            lane.free_at_millis = arrives;
            lane.inflight.push((assignment.index, arrives));
        }
    }

    /// Advances to the next arrival and delivers everything due.
    fn advance(&mut self) -> bool {
        let Some(next) = self
            .lanes
            .values()
            .flat_map(|lane| lane.inflight.iter().map(|(_, at)| *at))
            .min()
        else {
            return false;
        };
        self.now_millis = self.now_millis.max(next);

        let due: Vec<(LaneId, ChunkIndex)> = self
            .lanes
            .iter()
            .flat_map(|(id, lane)| {
                let id = *id;
                lane.inflight
                    .iter()
                    .filter(|(_, at)| *at <= self.now_millis)
                    .map(move |(index, _)| (id, *index))
            })
            .collect();

        for (lane_id, index) in due {
            for lane in self.lanes.values_mut() {
                lane.inflight.retain(|(held, _)| *held != index);
            }
            if self.scheduler.on_delivered(index, lane_id, self.now_millis)
                && let Some(lane) = self.lanes.get_mut(&lane_id)
            {
                lane.delivered += 1;
            }
        }
        true
    }

    fn run(&mut self) -> u64 {
        for _ in 0..100_000 {
            if self.scheduler.is_complete() {
                return self.now_millis;
            }
            self.dispatch_all();
            if !self.advance() {
                break;
            }
        }
        assert!(self.scheduler.is_complete(), "simulation did not finish");
        self.now_millis
    }

    fn delivered_by(&self, lane: LaneId) -> u32 {
        self.lanes
            .get(&lane)
            .map(|l| l.delivered)
            .unwrap_or_default()
    }

    fn total_delivered(&self) -> u32 {
        self.lanes.values().map(|l| l.delivered).sum()
    }
}

const PAYLOAD: u64 = 64 * MIB as u64;

#[test]
fn a_single_lane_transfer_completes() {
    let mut simulation = Simulation::new(PAYLOAD, &[(100_000, 4_000)]);
    simulation.run();
    assert_eq!(simulation.total_delivered(), 64);
    assert_eq!(simulation.scheduler.progress().delivered_bytes, PAYLOAD);
}

#[test]
fn two_equal_lanes_each_carry_a_real_share() {
    let mut simulation = Simulation::new(PAYLOAD, &[(100_000, 4_000), (100_000, 4_000)]);
    simulation.run();

    let first = simulation.delivered_by(simulation.lane(0));
    let second = simulation.delivered_by(simulation.lane(1));
    assert_eq!(first + second, 64);
    // The share test: a lane at zero chunks would mean failover, not striping.
    assert!(
        first * 100 / 64 > 10,
        "first lane carried only {first} chunks"
    );
    assert!(
        second * 100 / 64 > 10,
        "second lane carried only {second} chunks"
    );
    assert!(
        first.abs_diff(second) <= 8,
        "lopsided split: {first} vs {second}"
    );
}

#[test]
fn two_independent_lanes_finish_sooner_than_one() {
    let alone = Simulation::new(PAYLOAD, &[(100_000, 4_000)]).run();
    let together = Simulation::new(PAYLOAD, &[(100_000, 4_000), (100_000, 4_000)]).run();

    let speedup = alone as f64 / together as f64;
    assert!(
        speedup > 1.7,
        "two lanes gave only {speedup:.2}x (alone {alone}ms, {together}ms)"
    );
}

#[test]
fn three_lanes_aggregate_further() {
    let two = Simulation::new(PAYLOAD, &[(100_000, 4_000), (100_000, 4_000)]).run();
    let three = Simulation::new(
        PAYLOAD,
        &[(100_000, 4_000), (100_000, 4_000), (100_000, 4_000)],
    )
    .run();
    assert!(
        three < two,
        "third lane did not help: {two}ms then {three}ms"
    );
}

#[test]
fn a_far_slower_lane_receives_proportionally_little_and_never_hurts() {
    let fast_alone = Simulation::new(PAYLOAD, &[(400_000, 1_000)]).run();

    let mut mixed = Simulation::new(PAYLOAD, &[(400_000, 1_000), (20_000, 40_000)]);
    let together = mixed.run();

    let fast = mixed.delivered_by(mixed.lane(0));
    let slow = mixed.delivered_by(mixed.lane(1));
    assert_eq!(fast + slow, 64);
    assert!(fast > slow * 4, "slow lane took too much: {fast} vs {slow}");
    assert!(
        together <= fast_alone,
        "adding a slow lane made it worse: {fast_alone}ms then {together}ms"
    );
}

#[test]
fn every_chunk_is_delivered_exactly_once_across_all_lanes() {
    let mut simulation = Simulation::new(
        PAYLOAD,
        &[(200_000, 2_000), (60_000, 12_000), (20_000, 40_000)],
    );
    simulation.run();
    assert_eq!(simulation.total_delivered(), 64);
    assert!(simulation.scheduler.bitmap().is_complete());
    assert_eq!(simulation.scheduler.bitmap().missing().count(), 0);
}

#[test]
fn a_lane_lost_mid_transfer_hands_its_chunks_back() {
    let mut simulation = Simulation::new(PAYLOAD, &[(100_000, 4_000), (100_000, 4_000)]);
    simulation.dispatch_all();
    simulation.advance();

    let doomed = simulation.lane(1);
    let orphaned = simulation.scheduler.remove_lane(doomed);
    assert!(orphaned > 0, "the lane held no chunks to hand back");
    simulation.lanes.remove(&doomed);

    simulation.run();
    assert!(simulation.scheduler.bitmap().is_complete());
    assert_eq!(simulation.scheduler.bitmap().missing().count(), 0);
    assert_eq!(simulation.scheduler.progress().delivered_bytes, PAYLOAD);
}

#[test]
fn a_stalled_chunk_moves_to_a_different_lane() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    let first = LaneId::new(1);
    let second = LaneId::new(2);
    scheduler.add_lane(first, &profile_with(100_000, 4_000));
    scheduler.add_lane(second, &profile_with(100_000, 4_000));

    let sent = scheduler.next_assignment(0).unwrap();
    assert!(
        scheduler.detect_stalls(100).is_empty(),
        "reassigned before the deadline"
    );

    assert_eq!(scheduler.detect_stalls(60_000), vec![sent.index]);
    let retry = scheduler.next_assignment(60_000).unwrap();
    assert_eq!(retry.index, sent.index);
    assert_ne!(
        retry.lane, sent.lane,
        "handed the chunk back to the lane that stalled"
    );
}

#[test]
fn a_chunk_that_arrives_twice_is_counted_once() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    let lane = LaneId::new(1);
    scheduler.add_lane(lane, &profile_with(100_000, 4_000));

    let sent = scheduler.next_assignment(0).unwrap();
    assert!(scheduler.on_delivered(sent.index, lane, 100));
    assert!(!scheduler.on_delivered(sent.index, lane, 200));
    assert_eq!(scheduler.progress().completed_chunks, 1);
    assert_eq!(scheduler.progress().delivered_bytes, u64::from(CHUNK_LEN));
}

#[test]
fn a_chunk_delivered_by_one_lane_retires_the_copy_on_another() {
    let layout = ChunkLayout::new(2 * MIB as u64).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    let first = LaneId::new(1);
    let second = LaneId::new(2);
    scheduler.add_lane(first, &profile_with(100_000, 4_000));
    scheduler.add_lane(second, &profile_with(100_000, 4_000));

    let a = scheduler.next_assignment(0).unwrap();
    let b = scheduler.next_assignment(0).unwrap();
    assert_ne!(a.lane, b.lane);

    // With both chunks out, the tail rule starts duplicating rather than idling.
    let duplicate = scheduler.next_assignment(0).unwrap();
    assert!(duplicate.duplicate);
    assert_ne!(duplicate.lane, a.lane);

    scheduler.on_delivered(a.index, a.lane, 100);
    scheduler.on_delivered(b.index, b.lane, 100);
    assert!(scheduler.is_complete());
}

#[test]
fn the_tail_is_never_duplicated_while_fresh_work_remains() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    scheduler.add_lane(LaneId::new(1), &profile_with(100_000, 4_000));
    scheduler.add_lane(LaneId::new(2), &profile_with(100_000, 4_000));

    let mut seen = std::collections::BTreeSet::new();
    while let Some(assignment) = scheduler.next_assignment(0) {
        assert!(
            !assignment.duplicate,
            "duplicated {:?} with 64 chunks queued",
            assignment.index
        );
        assert!(
            seen.insert(assignment.index),
            "sent {:?} twice",
            assignment.index
        );
    }
    let allowed: u32 = scheduler.lane_stats().map(|lane| lane.inflight_limit).sum();
    assert_eq!(seen.len() as u32, allowed, "in-flight limit not respected");
}

/// Queue depth is measured in time, not in chunks. Four chunks on a 1.6 Mbps link is twenty
/// seconds of work sitting behind the one lane the tail will wait on.
#[test]
fn a_slow_lane_queues_less_work_than_a_fast_one() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    let wifi = LaneId::new(1);
    let bluetooth = LaneId::new(2);
    scheduler.add_lane(wifi, &LaneProfile::WIFI_INFRA);
    scheduler.add_lane(bluetooth, &LaneProfile::BLUETOOTH_CLASSIC);

    let limits: std::collections::BTreeMap<LaneId, u32> = scheduler
        .lane_stats()
        .map(|lane| (lane.lane, lane.inflight_limit))
        .collect();
    assert_eq!(limits[&bluetooth], 1);
    assert_eq!(limits[&wifi], ChunkScheduler::DEFAULT_MAX_INFLIGHT_PER_LANE);
}

#[test]
fn each_lane_is_credited_with_exactly_what_it_delivered() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    let fast = LaneId::new(1);
    let slow = LaneId::new(2);
    scheduler.add_lane(fast, &profile_with(400_000, 4_000));
    scheduler.add_lane(slow, &profile_with(1_600, 30_000));

    let mut expected: std::collections::BTreeMap<LaneId, (u64, u32)> =
        std::collections::BTreeMap::new();
    let mut clock = 0;
    while !scheduler.is_complete() {
        let Some(assignment) = scheduler.next_assignment(clock) else {
            break;
        };
        clock += 1;
        if scheduler.on_delivered(assignment.index, assignment.lane, clock) {
            let entry = expected.entry(assignment.lane).or_default();
            entry.0 += u64::from(assignment.len);
            entry.1 += 1;
        }
    }

    for lane in scheduler.lane_stats() {
        let (bytes, chunks) = expected.get(&lane.lane).copied().unwrap_or_default();
        assert_eq!(lane.delivered_bytes, bytes, "{:?} bytes", lane.lane);
        assert_eq!(lane.delivered_chunks, chunks, "{:?} chunks", lane.lane);
    }
    let carried: u64 = scheduler
        .lane_stats()
        .map(|lane| lane.delivered_bytes)
        .sum();
    assert_eq!(carried, scheduler.delivered_bytes());
    let chunks: u32 = scheduler
        .lane_stats()
        .map(|lane| lane.delivered_chunks)
        .sum();
    assert_eq!(chunks, scheduler.progress().completed_chunks);
}

/// A lane's measured bandwidth has to survive being queued behind its own backlog.
///
/// Charging a chunk for the time it spent waiting rather than the time it spent sending divides
/// every figure by the queue depth, and that figure is the one the benchmark prints.
#[test]
fn a_lanes_measured_bandwidth_is_not_divided_by_its_queue_depth() {
    const TRUTH_KBPS: u32 = 200_000;
    let mut simulation = Simulation::new(PAYLOAD, &[(TRUTH_KBPS, 4_000)]);
    simulation.run();

    let measured = simulation
        .scheduler
        .lane_stats()
        .next()
        .map(|lane| lane.bandwidth_kbps)
        .unwrap();
    let ratio = f64::from(measured) / f64::from(TRUTH_KBPS);
    assert!(
        (0.75..1.35).contains(&ratio),
        "measured {measured} kbps against a true {TRUTH_KBPS}, ratio {ratio:.2}"
    );
}

/// Round trip time is not transmission time. Conflating them charges a slow lane for its own
/// serialisation twice, once in `drain_micros` and again as half an RTT.
#[test]
fn chunk_timings_do_not_masquerade_as_round_trip_time() {
    let mut simulation = Simulation::new(PAYLOAD, &[(1_600, 30_000)]);
    simulation.run();

    let rtt = simulation
        .scheduler
        .lane_stats()
        .next()
        .map(|lane| lane.rtt_micros)
        .unwrap();
    assert_eq!(
        rtt,
        LaneProfile::BLUETOOTH_CLASSIC.rtt_hint_micros,
        "nothing measured a round trip, so the hint should still stand"
    );
}

/// The share each medium carries is what the report is for, so it has to survive the round trip
/// through the counters, not just through the scheduler's private state.
#[test]
fn two_equal_lanes_are_credited_with_a_real_share_each() {
    let mut simulation = Simulation::new(PAYLOAD, &[(200_000, 4_000), (200_000, 4_000)]);
    simulation.run();
    let shares: Vec<u32> = simulation
        .scheduler
        .lane_stats()
        .map(|lane| lane.delivered_chunks)
        .collect();
    let total: u32 = shares.iter().sum();
    assert_eq!(total, simulation.scheduler.progress().completed_chunks);
    for share in shares {
        assert!(share * 10 > total, "a lane at {share}/{total} is failover");
    }
}

#[test]
fn a_resumed_transfer_sends_only_what_is_missing() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut bitmap = ChunkBitmap::new(layout.count());
    for index in 0..60 {
        bitmap.insert(ChunkIndex::new(index));
    }
    let mut scheduler = ChunkScheduler::resuming(layout, bitmap);
    scheduler.add_lane(LaneId::new(1), &profile_with(100_000, 4_000));

    let mut assigned = Vec::new();
    while let Some(assignment) = scheduler.next_assignment(0) {
        if assignment.duplicate {
            break;
        }
        assigned.push(assignment.index);
    }
    assert!(
        assigned.iter().all(|index| index.as_u32() >= 60),
        "resent a completed chunk"
    );
    assert_eq!(scheduler.progress().completed_chunks, 60);
}

#[test]
fn the_weakest_lane_is_the_probe_candidate_and_needs_a_rival() {
    let layout = ChunkLayout::new(PAYLOAD).unwrap();
    let mut scheduler = ChunkScheduler::new(layout);
    scheduler.add_lane(LaneId::new(1), &profile_with(400_000, 1_000));
    assert_eq!(
        scheduler.weakest_working_lane(),
        None,
        "probed with nothing to compare against"
    );

    scheduler.add_lane(LaneId::new(2), &profile_with(20_000, 40_000));
    assert_eq!(scheduler.weakest_working_lane(), Some(LaneId::new(2)));

    scheduler.set_parked(LaneId::new(2), true);
    assert_eq!(scheduler.weakest_working_lane(), None);
    assert_eq!(scheduler.working_lane_count(), 1);
}

// ---------------------------------------------------------------- probe

/// Runs one full measurement cycle, reporting the bytes delivered in each window.
fn probe_cycle(
    probe: &mut MarginalContributionProbe,
    lane: LaneId,
    clock: &mut u64,
    delivered: &mut u64,
    with_lane: u64,
    without_lane: u64,
) -> ProbeAction {
    *clock += MarginalContributionProbe::DEFAULT_INTERVAL_MILLIS;
    assert_eq!(
        probe.poll(*clock, *delivered, Some(lane)),
        ProbeAction::Wait
    );

    *clock += MarginalContributionProbe::DEFAULT_WINDOW_MILLIS;
    *delivered += with_lane;
    assert_eq!(
        probe.poll(*clock, *delivered, Some(lane)),
        ProbeAction::SuspendForMeasurement(lane)
    );
    assert!(probe.is_measuring());

    *clock += MarginalContributionProbe::DEFAULT_WINDOW_MILLIS;
    *delivered += without_lane;
    probe.poll(*clock, *delivered, Some(lane))
}

#[test]
fn a_lane_that_makes_the_transfer_faster_when_removed_is_parked_after_two_verdicts() {
    let lane = LaneId::new(2);
    let mut probe = MarginalContributionProbe::new(0);
    let mut clock = 0;
    let mut delivered = 0;

    assert_eq!(
        probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 1_400),
        ProbeAction::Restore(lane),
        "parked on a single window"
    );
    assert_eq!(
        probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 1_400),
        ProbeAction::Park(lane)
    );
}

#[test]
fn a_lane_that_is_earning_its_place_is_always_restored() {
    let lane = LaneId::new(2);
    let mut probe = MarginalContributionProbe::new(0);
    let mut clock = 0;
    let mut delivered = 0;

    for _ in 0..5 {
        assert_eq!(
            probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 2_000, 1_100),
            ProbeAction::Restore(lane)
        );
    }
}

#[test]
fn a_gain_inside_the_noise_threshold_is_not_acted_on() {
    let lane = LaneId::new(2);
    let mut probe = MarginalContributionProbe::new(0);
    let mut clock = 0;
    let mut delivered = 0;

    for _ in 0..3 {
        assert_eq!(
            probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 1_020),
            ProbeAction::Restore(lane),
            "acted on a two percent difference"
        );
    }
}

#[test]
fn one_bad_window_followed_by_a_good_one_does_not_park() {
    let lane = LaneId::new(2);
    let mut probe = MarginalContributionProbe::new(0);
    let mut clock = 0;
    let mut delivered = 0;

    probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 1_400);
    probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 900);
    assert_eq!(
        probe_cycle(&mut probe, lane, &mut clock, &mut delivered, 1_000, 1_400),
        ProbeAction::Restore(lane),
        "the streak was not reset by the good window"
    );
}

#[test]
fn the_probe_stays_idle_without_a_candidate() {
    let mut probe = MarginalContributionProbe::new(0);
    for step in 0..10 {
        assert_eq!(
            probe.poll(step * 10_000, step * 1_000, None),
            ProbeAction::Wait
        );
    }
    assert!(!probe.is_measuring());
}
