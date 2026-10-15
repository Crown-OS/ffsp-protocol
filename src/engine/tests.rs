#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::*;
use crate::control::TransferId;
use crate::device::{Capabilities, DeviceClass, DeviceId};
use crate::engine::AdoptedLane;
use crate::link::{LinkRecord, LinkStore, LinkStoreError, generate_identity};
use crate::medium::{Medium, loopback_channel_pair};
use crate::platform::PowerHint;
use crate::transfer::CHUNK_LEN;
use crate::wire::LaneId;

/// Keeps pairing records in memory, because a test has no keyring.
#[derive(Debug, Default)]
struct MemoryLinks {
    records: Mutex<BTreeMap<DeviceId, LinkRecord>>,
}

impl LinkStore for MemoryLinks {
    fn load(&self, device: &DeviceId) -> Result<Option<LinkRecord>, LinkStoreError> {
        Ok(self
            .records
            .lock()
            .map_err(|_| LinkStoreError::Unavailable("poisoned".to_owned()))?
            .get(device)
            .cloned())
    }

    fn store(&self, record: &LinkRecord) -> Result<(), LinkStoreError> {
        self.records
            .lock()
            .map_err(|_| LinkStoreError::Unavailable("poisoned".to_owned()))?
            .insert(record.device, record.clone());
        Ok(())
    }

    fn remove(&self, device: &DeviceId) -> Result<(), LinkStoreError> {
        self.records
            .lock()
            .map_err(|_| LinkStoreError::Unavailable("poisoned".to_owned()))?
            .remove(device);
        Ok(())
    }

    fn all(&self) -> Result<Vec<LinkRecord>, LinkStoreError> {
        Ok(self
            .records
            .lock()
            .map_err(|_| LinkStoreError::Unavailable("poisoned".to_owned()))?
            .values()
            .cloned()
            .collect())
    }
}

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

/// A scratch directory that removes itself, so the crate takes no dev-dependency for it.
///
/// `/tmp` is a RAM filesystem on this machine, which is fine for the megabytes here and is not
/// where a multi-gigabyte benchmark should point.
#[derive(Debug)]
struct Scratch {
    path: std::path::PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let ordinal = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ffsp-engine-{}-{name}-{ordinal}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn join(&self, name: &str) -> std::path::PathBuf {
        self.path.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct Rig {
    sender: Engine,
    sender_events: EngineEvents,
    receiver: Engine,
    receiver_events: EngineEvents,
    _root: Scratch,
    source: std::path::PathBuf,
    destination: std::path::PathBuf,
}

/// Two engines in one process, joined by in-memory lanes.
///
/// Everything below the lane is what the platform crates supply; everything above it is what has
/// to be right before any radio is worth pointing at.
fn rig(payload: &[u8]) -> Rig {
    let root = Scratch::new("rig");
    let source = root.join("payload.bin");
    std::fs::write(&source, payload).unwrap();
    let destination = root.join("inbox");
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::create_dir_all(root.join("outbox")).unwrap();

    let tuning = Tuning::for_power(PowerHint::Unconstrained);
    let capabilities = Capabilities::WIFI_INFRA | Capabilities::USB_ACCESSORY;
    let sender_config = EngineConfig::new(
        generate_identity().unwrap(),
        Arc::new(MemoryLinks::default()),
        root.join("outbox"),
    )
    .with_name("laptop")
    .with_class(DeviceClass::Laptop)
    .with_capabilities(capabilities)
    .with_tuning(tuning);
    let receiver_config = EngineConfig::new(
        generate_identity().unwrap(),
        Arc::new(MemoryLinks::default()),
        destination.clone(),
    )
    .with_name("phone")
    .with_class(DeviceClass::Phone)
    .with_capabilities(capabilities)
    .with_tuning(tuning);

    let (sender, sender_events) = start(sender_config);
    let (receiver, receiver_events) = start(receiver_config);

    Rig {
        sender,
        sender_events,
        receiver,
        receiver_events,
        _root: root,
        source,
        destination,
    }
}

const MEDIUMS: [Medium; 3] = [
    Medium::WifiInfra,
    Medium::UsbAccessory,
    Medium::BluetoothClassic,
];

/// Hands both engines the two ends of one in-memory medium.
fn join(rig: &Rig, medium: Medium, label: &str, adopt: Option<(DeviceId, DeviceId, LaneId)>) {
    let (near, far) = loopback_channel_pair(LaneId::new(1), 8 * 1024 * 1024);
    let (sender_adopt, receiver_adopt) = match adopt {
        Some((receiver_peer, sender_peer, lane)) => (
            Some(AdoptedLane {
                peer: receiver_peer,
                lane,
            }),
            Some(AdoptedLane {
                peer: sender_peer,
                lane,
            }),
        ),
        None => (None, None),
    };
    rig.sender
        .send(EngineCommand::AttachLane {
            medium,
            endpoint: format!("loopback:{label}"),
            channel: Box::new(near),
            initiate: true,
            adopt: sender_adopt,
        })
        .unwrap();
    rig.receiver
        .send(EngineCommand::AttachLane {
            medium,
            endpoint: format!("loopback:{label}"),
            channel: Box::new(far),
            initiate: false,
            adopt: receiver_adopt,
        })
        .unwrap();
}

async fn wait_for_link(events: &mut EngineEvents) -> DeviceId {
    loop {
        match events.next().await.expect("engine stopped") {
            EngineEvent::LinkSettled { device, accepted } => {
                assert!(accepted);
                return device;
            }
            EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                panic!("engine failed: {reason}")
            }
            EngineEvent::Diagnostic(Diagnostic::LaneFailed { reason, .. }) => {
                panic!("lane failed: {reason}")
            }
            _ => {}
        }
    }
}

/// Waits for the peer to name the lane it reserved for a medium.
async fn wait_for_invite(events: &mut EngineEvents, medium: Medium) -> LaneId {
    loop {
        match events.next().await.expect("engine stopped") {
            EngineEvent::LaneInvited {
                lane,
                medium: invited,
                ..
            } if invited == medium => return lane,
            EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                panic!("engine failed: {reason}")
            }
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_pair_over_a_single_lane() {
    let payload = payload_of(4096);
    let mut rig = rig(&payload);
    join(&rig, Medium::WifiInfra, "0", None);
    let peer_of_sender = wait_for_link(&mut rig.sender_events).await;
    let peer_of_receiver = wait_for_link(&mut rig.receiver_events).await;
    assert_ne!(peer_of_sender, peer_of_receiver);
}

/// The whole point, end to end: an offer, a verification tree, striped chunks, and a file on the
/// other side that is byte for byte the one that went in.
async fn transfer_over(lane_count: usize, payload: Vec<u8>) {
    transfer_over_reporting(lane_count, payload).await;
}

async fn transfer_over_reporting(
    lane_count: usize,
    payload: Vec<u8>,
) -> crate::metrics::TransferReport {
    let mut rig = rig(&payload);
    join(&rig, MEDIUMS[0], "0", None);
    let receiver_peer = wait_for_link(&mut rig.sender_events).await;
    let sender_peer = wait_for_link(&mut rig.receiver_events).await;

    // Later mediums join the session the first one established, at identifiers the initiator
    // chooses. Both ends numbering independently is what breaks when the hardware comes up in a
    // different order on each side.
    for (ordinal, medium) in MEDIUMS.iter().copied().enumerate().take(lane_count).skip(1) {
        rig.sender
            .send(EngineCommand::InviteLane {
                peer: receiver_peer,
                medium,
                endpoint: format!("loopback:{ordinal}"),
            })
            .unwrap();
        let lane = wait_for_invite(&mut rig.sender_events, medium).await;
        let echoed = wait_for_invite(&mut rig.receiver_events, medium).await;
        assert_eq!(lane, echoed, "the two ends disagreed about the lane");
        join(
            &rig,
            medium,
            &ordinal.to_string(),
            Some((receiver_peer, sender_peer, lane)),
        );
    }

    rig.sender
        .send(EngineCommand::Offer {
            peer: receiver_peer,
            path: rig.source.clone(),
        })
        .unwrap();

    // The receiving side agrees as soon as it is told what is on offer.
    let accepting = tokio::spawn({
        let receiver = rig.receiver.clone();
        let mut events = rig.receiver_events;
        async move {
            let report;
            loop {
                match events.next().await.expect("engine stopped") {
                    EngineEvent::TransferOffered { device, .. } => {
                        receiver
                            .send(EngineCommand::Accept {
                                peer: device,
                                transfer: TransferId::new(1),
                            })
                            .unwrap();
                    }
                    EngineEvent::TransferReport { report: got, .. } => {
                        report = *got;
                        break;
                    }
                    EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                        panic!("receiver failed: {reason}")
                    }
                    _ => {}
                }
            }
            report
        }
    });

    let sender_report = tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            match rig.sender_events.next().await.expect("engine stopped") {
                EngineEvent::TransferReport { report, .. } => break *report,
                EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                    panic!("sender failed: {reason}")
                }
                _ => {}
            }
        }
    })
    .await
    .expect("the transfer did not finish");

    let receiver_report = tokio::time::timeout(std::time::Duration::from_secs(60), accepting)
        .await
        .expect("the receiver did not finish")
        .unwrap();

    let landed = std::fs::read(rig.destination.join("payload.bin")).unwrap();
    assert_eq!(landed.len(), payload.len());
    assert_eq!(landed, payload, "the payload did not survive the trip");

    assert_eq!(sender_report.total_bytes, payload.len() as u64);
    assert_eq!(receiver_report.total_bytes, payload.len() as u64);
    assert_eq!(
        receiver_report.role,
        crate::metrics::TransferRole::Receiving
    );

    let carried: u64 = receiver_report
        .lanes
        .iter()
        .map(|lane| lane.received_bytes)
        .sum();
    assert!(
        carried >= payload.len() as u64,
        "lanes accounted for {carried} of {} bytes",
        payload.len()
    );
    receiver_report
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_lane_carries_a_multi_chunk_payload() {
    transfer_over(1, payload_of(4 * CHUNK_LEN as usize + 12_345)).await;
}

/// Every medium has to be credited with what it carried, or the report cannot tell striping
/// from failover.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_medium_is_credited_separately() {
    let payload = payload_of(8 * CHUNK_LEN as usize);
    let report = transfer_over_reporting(3, payload).await;
    assert_eq!(report.lanes.len(), 3);
    let mediums: std::collections::BTreeSet<_> =
        report.lanes.iter().map(|lane| lane.medium).collect();
    assert_eq!(mediums.len(), 3, "lanes collapsed onto one medium");
    let carried: u64 = report.lanes.iter().map(|lane| lane.received_bytes).sum();
    assert!(carried >= report.total_bytes);
    assert!(report.to_json().contains("\"received_bytes\""));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_mediums_carry_one_payload_between_them() {
    transfer_over(3, payload_of(12 * CHUNK_LEN as usize + 7)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_payload_smaller_than_one_chunk_still_arrives() {
    transfer_over(2, payload_of(4096)).await;
}

/// The rig brings Bluetooth up first, so a one-medium run is a Bluetooth run — and a profile
/// whose transfer budget differs from Wi-Fi's is where a framing assumption would show.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_single_bluetooth_lane_carries_a_whole_payload() {
    let payload = payload_of(8 * CHUNK_LEN as usize);
    let mut rig = rig(&payload);
    join(&rig, Medium::BluetoothClassic, "0", None);
    let receiver_peer = wait_for_link(&mut rig.sender_events).await;
    wait_for_link(&mut rig.receiver_events).await;

    rig.sender
        .send(EngineCommand::Offer {
            peer: receiver_peer,
            path: rig.source.clone(),
        })
        .unwrap();

    let receiver = rig.receiver.clone();
    let mut events = rig.receiver_events;
    let receiving = tokio::spawn(async move {
        loop {
            match events.next().await.expect("engine stopped") {
                EngineEvent::TransferOffered { device, .. } => {
                    receiver
                        .send(EngineCommand::Accept {
                            peer: device,
                            transfer: TransferId::new(1),
                        })
                        .unwrap();
                }
                EngineEvent::TransferReport { .. } => break,
                EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }) => {
                    panic!("receiver failed: {reason}")
                }
                _ => {}
            }
        }
    });

    tokio::time::timeout(std::time::Duration::from_secs(30), receiving)
        .await
        .expect("a single Bluetooth lane never finished")
        .unwrap();

    let landed = std::fs::read(rig.destination.join("payload.bin")).unwrap();
    assert_eq!(landed, payload);
}
