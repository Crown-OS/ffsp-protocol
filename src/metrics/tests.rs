#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::crypto::CipherSuite;
use crate::medium::Medium;
use crate::transfer::CHUNK_LEN;
use crate::wire::LaneId;

const MIB: u64 = 1024 * 1024;

fn meter(lane: u16, medium: Medium) -> LaneMeter {
    LaneMeter::new(LaneId::new(lane), medium, "test", 0)
}

#[test]
fn a_rate_is_bytes_over_time_and_survives_a_zero_window() {
    assert_eq!(rate_kbps(0, 0), 0);
    assert_eq!(rate_kbps(1_000_000, 0), 0);
    // 1 MB in 1 s is 8 Mbit/s.
    assert_eq!(rate_kbps(1_000_000, 1_000_000), 8_000);
}

#[test]
fn a_window_measures_the_window_and_not_the_whole_transfer() {
    let mut meter = meter(1, Medium::WifiInfra);
    meter.record_received_frame(MIB);
    let first = meter.sample(1_000_000, TransferRole::Receiving);
    assert_eq!(first.kbps, rate_kbps(MIB, 1_000_000));

    // Nothing arrives in the second window.
    let second = meter.sample(2_000_000, TransferRole::Receiving);
    assert_eq!(second.kbps, 0);
    assert_eq!(second.cumulative_bytes, MIB);
    // The peak remembers the good window.
    assert_eq!(meter.peak_kbps(), first.kbps);
}

#[test]
fn a_duplicate_is_counted_as_waste_rather_than_as_progress() {
    let mut meter = meter(1, Medium::UsbAccessory);
    meter.record_received_chunk(MIB, false);
    meter.record_received_chunk(MIB, true);
    assert_eq!(meter.received_chunks(), 1);
    assert_eq!(meter.duplicate_bytes(), MIB);
}

#[test]
fn round_trip_percentiles_need_no_floating_point() {
    let mut summary = RttSummary::default();
    for micros in [10u32, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
        summary.observe(micros);
    }
    assert_eq!(summary.min(), Some(10));
    assert_eq!(summary.max(), Some(100));
    assert_eq!(summary.mean(), Some(55));
    assert_eq!(summary.percentile(50), Some(50));
    assert_eq!(summary.percentile(95), Some(100));
    assert_eq!(summary.count(), 10);
}

#[test]
fn a_timeline_keeps_each_lane_apart_and_stays_bounded() {
    let mut timeline = Timeline::new(4);
    for at in 0..10u64 {
        for lane in [1u16, 2] {
            timeline.push(ThroughputSample {
                at_micros: at,
                lane: LaneId::new(lane),
                medium: Medium::WifiInfra,
                cumulative_bytes: at,
                kbps: 1,
            });
        }
    }
    assert_eq!(timeline.lanes().count(), 2);
    assert_eq!(timeline.samples().count(), 8);
}

#[test]
fn setup_phases_are_measured_separately_rather_than_lumped_together() {
    let mut setup = SetupTiming::new();
    setup.begin("bluetooth pair", 0);
    setup.end("bluetooth pair", 3_000_000);
    setup.begin("accessory switch", 3_000_000);
    setup.end("accessory switch", 3_400_000);
    setup.begin("chunk map", 3_400_000);

    assert_eq!(setup.elapsed_micros("bluetooth pair"), Some(3_000_000));
    assert_eq!(setup.elapsed_micros("accessory switch"), Some(400_000));
    assert_eq!(setup.elapsed_micros("chunk map"), None);
    assert_eq!(setup.phases().len(), 3);
}

fn report() -> TransferReport {
    let now = 8_000_000;
    let mut wifi = meter(1, Medium::WifiInfra);
    let mut usb = meter(2, Medium::UsbAccessory);
    let mut bluetooth = meter(3, Medium::BluetoothClassic);
    wifi.record_received_frame(300 * MIB);
    usb.record_received_frame(211 * MIB);
    bluetooth.record_received_frame(MIB);
    wifi.observe_rtt(4_000);
    usb.observe_rtt(500);
    bluetooth.observe_rtt(30_000);

    let lanes = [&wifi, &usb, &bluetooth]
        .into_iter()
        .map(|meter| LaneSummary::new(meter, None, now, TransferRole::Receiving))
        .collect();

    TransferReport {
        role: TransferRole::Receiving,
        name: "holiday.mp4".to_owned(),
        total_bytes: 512 * MIB,
        total_chunks: 512,
        wall_micros: now,
        conditions: RunConditions {
            max_bulk_lanes: 4,
            max_inflight_per_lane: 4,
            buffer_pool_size: 32,
            buffer_footprint_bytes: 34_079_360,
            probing: true,
            cipher_suite: CipherSuite::Aes256Gcm,
            aes_hardware: true,
            chunk_len: CHUNK_LEN,
        },
        lanes,
        setup: SetupTiming::new(),
        timeline: Timeline::default(),
        buffer_starvations: 0,
        abandoned_chunks: 2,
    }
}

#[test]
fn the_lane_shares_of_a_report_account_for_the_whole_payload() {
    let report = report();
    let total: u64 = report.lanes.iter().map(|lane| lane.carried_bytes).sum();
    assert_eq!(total, report.total_bytes);
    let shares: u32 = report
        .lanes
        .iter()
        .map(|lane| lane.share_permille(report.total_bytes))
        .sum();
    // Each share truncates, so the sum falls short by at most one per lane.
    let floor = 1_000 - u32::try_from(report.lanes.len()).unwrap();
    assert!(
        (floor..=1_000).contains(&shares),
        "shares summed to {shares}, not within {floor}..=1000"
    );
}

/// The headline claim of striping is that three mediums beat the best one, and it has to be
/// checkable from the report rather than asserted in prose.
#[test]
fn the_report_states_what_striping_bought() {
    let report = report();
    assert!(report.aggregate_kbps() > report.best_lane_kbps());
    assert!(
        report.speedup_permille() > 1_000,
        "speedup {} was not a gain",
        report.speedup_permille()
    );
}

#[test]
fn a_report_round_trips_through_json_and_csv_without_losing_a_lane() {
    let report = report();
    let json = report.to_json();
    assert!(json.starts_with('{') && json.ends_with('}'));
    for medium in ["Wi-Fi", "USB", "Bluetooth"] {
        assert!(json.contains(medium), "{medium} missing from {json}");
    }
    assert!(json.contains("\"aes_hardware\":true"));

    let csv = report.to_csv();
    assert_eq!(csv.lines().count(), 1 + report.lanes.len());
}

/// A combined run hands Bluetooth a chunk or two, so its share there is a sample too small to
/// quote. Only a standalone run says what the medium can do.
#[test]
fn calibration_compares_each_medium_alone_against_all_of_them_together() {
    let calibration = CalibrationReport {
        label: "512 MiB".into(),
        standalone: vec![
            StandaloneRun {
                medium: Medium::WifiInfra,
                bytes: 512 * MIB,
                wall_micros: 10_737_000,
            },
            StandaloneRun {
                medium: Medium::UsbAccessory,
                bytes: 512 * MIB,
                wall_micros: 16_000_000,
            },
            StandaloneRun {
                medium: Medium::BluetoothClassic,
                bytes: 512 * MIB,
                wall_micros: 2_684_354_000,
            },
        ],
        combined: Some(StandaloneRun {
            medium: Medium::WifiInfra,
            bytes: 512 * MIB,
            wall_micros: 6_333_000,
        }),
    };
    assert_eq!(
        calibration.best_standalone().map(|run| run.medium),
        Some(Medium::WifiInfra)
    );
    let speedup = calibration.speedup_permille().unwrap();
    assert!((1_600..1_800).contains(&speedup), "speedup was {speedup}");
    assert!(calibration.to_json().contains("speedup_permille"));
}

/// A sender never receives a chunk, so a report fixed to the receiving direction shows a row of
/// zeros on the sending end — which reads as "this medium carried nothing" rather than "this
/// figure is measured at the other end".
#[test]
fn the_sending_end_reports_what_it_sent() {
    let mut meter = meter(1, Medium::WifiInfra);
    for _ in 0..4 {
        meter.record_sent_frame(MIB);
        meter.record_sent_chunk();
    }
    let sample = meter.sample(1_000_000, TransferRole::Sending);
    assert_eq!(sample.cumulative_bytes, 4 * MIB);
    assert!(sample.kbps > 0);

    let summary = LaneSummary::new(&meter, None, 1_000_000, TransferRole::Sending);
    assert_eq!(summary.carried_bytes, 4 * MIB);
    assert_eq!(summary.carried_chunks, 4);
    assert!(
        summary.mean_kbps > 0,
        "the sending end reported no throughput"
    );
    assert_eq!(summary.share_permille(4 * MIB), 1_000);

    // The same meter, read as a receiver, honestly reports nothing arrived.
    let mirrored = LaneSummary::new(&meter, None, 1_000_000, TransferRole::Receiving);
    assert_eq!(mirrored.carried_bytes, 0);
}
