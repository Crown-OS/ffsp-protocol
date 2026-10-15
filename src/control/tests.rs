#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use blake3::hazmat::ChainingValue;

use super::*;
use crate::medium::Medium;
use crate::transfer::{ChunkBitmap, FileEntry, Manifest, ResumeState};
use crate::wire::{ChunkIndex, LaneId};

fn manifest() -> Manifest {
    Manifest::new(vec![
        FileEntry::new("holiday.mp4".to_owned(), 4_294_967_296, blake3::hash(b"a")).unwrap(),
        FileEntry::new("notes/today.md".to_owned(), 1_024, blake3::hash(b"b")).unwrap(),
    ])
}

fn round_trip(message: &ControlMessage) {
    let encoded = message.to_bytes().unwrap();
    assert_eq!(&ControlMessage::parse(&encoded).unwrap(), message);
}

#[test]
fn every_control_message_round_trips() {
    let transfer = TransferId::new(7);
    round_trip(&ControlMessage::TransferOffer {
        transfer,
        manifest: manifest(),
    });
    round_trip(&ControlMessage::TransferAccept {
        transfer,
        resume: None,
    });
    round_trip(&ControlMessage::TransferAccept {
        transfer,
        resume: Some(ResumeState::new(
            blake3::hash(b"root"),
            8 * 1024 * 1024,
            ChunkBitmap::new(8),
        )),
    });
    round_trip(&ControlMessage::TransferDecline {
        transfer,
        reason: EndReason::NotEnoughSpace,
    });
    round_trip(&ControlMessage::TransferCancel {
        transfer,
        reason: EndReason::UserCancelled,
    });
    round_trip(&ControlMessage::ChunkMap(ChunkMapPage {
        transfer,
        first: ChunkIndex::new(256),
        chaining_values: vec![[9u8; 32], [10u8; 32]],
    }));
    round_trip(&ControlMessage::LaneInvite(LaneInvite {
        lane: LaneId::new(3),
        medium: Medium::UsbAccessory,
        endpoint: "usb:001/004".to_owned(),
    }));
    round_trip(&ControlMessage::TransferReady { transfer });
    round_trip(&ControlMessage::BenchmarkMark {
        label: "usb only".to_owned(),
    });
}

#[test]
fn an_opcode_from_a_newer_peer_names_itself() {
    let error = ControlMessage::parse(&[0x7E]).unwrap_err();
    assert!(matches!(error, ControlError::UnknownOpcode(0x7E)));
}

#[test]
fn the_platform_keeps_the_upper_half_of_the_opcode_space() {
    assert!(ControlMessage::is_engine_opcode(
        ControlOpcode::TransferOffer.as_byte()
    ));
    assert!(!ControlMessage::is_engine_opcode(
        ControlMessage::PLATFORM_OPCODE_BASE
    ));
}

#[test]
fn bytes_after_the_end_of_a_message_are_refused() {
    let mut encoded = ControlMessage::BenchmarkMark {
        label: "lan".to_owned(),
    }
    .to_bytes()
    .unwrap();
    encoded.push(0);
    assert!(matches!(
        ControlMessage::parse(&encoded).unwrap_err(),
        ControlError::TrailingBytes { trailing: 1 }
    ));
}

#[test]
fn a_truncated_message_is_refused_rather_than_guessed() {
    let encoded = ControlMessage::TransferOffer {
        transfer: TransferId::new(1),
        manifest: manifest(),
    }
    .to_bytes()
    .unwrap();
    for cut in 1..encoded.len() {
        assert!(ControlMessage::parse(&encoded[..cut]).is_err());
    }
}

/// The manifest is the one control message a hostile peer controls the contents of, so decoding
/// it is where a name that climbs out of the destination has to stop.
#[test]
fn a_manifest_entry_cannot_escape_the_destination_over_the_wire() {
    let mut encoded = vec![ControlOpcode::TransferOffer.as_byte()];
    encoded.extend_from_slice(&1u32.to_le_bytes());
    encoded.extend_from_slice(&1u32.to_le_bytes());
    let name = b"../../etc/passwd";
    encoded.extend_from_slice(&(name.len() as u16).to_le_bytes());
    encoded.extend_from_slice(name);
    encoded.extend_from_slice(&16u64.to_le_bytes());
    encoded.extend_from_slice(&[0u8; 32]);
    assert!(matches!(
        ControlMessage::parse(&encoded).unwrap_err(),
        ControlError::Transfer(_)
    ));
}

#[test]
fn pagination_carries_every_chaining_value_once() {
    let values: Vec<ChainingValue> = (0..600u32)
        .map(|index| {
            let mut value = [0u8; 32];
            value[..4].copy_from_slice(&index.to_le_bytes());
            value
        })
        .collect();
    let pages = ChunkMapPage::paginate(TransferId::new(2), &values);
    assert_eq!(pages.len(), 3);
    assert_eq!(pages[0].first, ChunkIndex::ZERO);
    assert_eq!(pages[1].first, ChunkIndex::new(CHUNK_MAP_PAGE_LEN));
    assert_eq!(pages[2].chaining_values.len(), 600 - 2 * 256);

    let rejoined: Vec<ChainingValue> = pages
        .iter()
        .flat_map(|page| page.chaining_values.clone())
        .collect();
    assert_eq!(rejoined, values);
}

/// A page has to fit the lane that is up first, which during bring-up is Bluetooth.
#[test]
fn a_chunk_map_page_fits_a_bluetooth_transfer() {
    let page = ChunkMapPage {
        transfer: TransferId::new(1),
        first: ChunkIndex::ZERO,
        chaining_values: vec![[0u8; 32]; CHUNK_MAP_PAGE_LEN as usize],
    };
    let encoded = ControlMessage::ChunkMap(page).to_bytes().unwrap();
    assert!(encoded.len() <= crate::medium::RFCOMM_MAX_TRANSFER);
}

/// The sender attributes throughput by the lane a chunk arrived on, which is not necessarily the
/// lane the acknowledgement came back over.
#[test]
fn an_acknowledgement_names_the_lane_that_carried_the_chunk() {
    let ack = ChunkAck::new(
        TransferId::new(4),
        vec![
            Delivery {
                index: ChunkIndex::new(11),
                lane: LaneId::new(2),
                arrived_at_micros: 1_500,
            },
            Delivery {
                index: ChunkIndex::new(12),
                lane: LaneId::new(3),
                arrived_at_micros: 1_900,
            },
        ],
    );
    let decoded = ChunkAck::parse(&ack.to_bytes().unwrap()).unwrap();
    assert_eq!(decoded, ack);
    assert_ne!(decoded.deliveries[0].lane, decoded.deliveries[1].lane);
}

#[test]
fn a_lane_report_round_trips() {
    let report = LaneReport {
        bandwidth_kbps: 384_000,
        rtt_micros: 3_200,
        delivered_bytes: 1 << 30,
        delivered_chunks: 1024,
    };
    assert_eq!(LaneReport::parse(&report.to_bytes()).unwrap(), report);
}

#[test]
fn a_keepalive_distinguishes_the_reply_from_the_question() {
    let ping = KeepAlive::ping(0xDEAD_BEEF_0000_0001);
    let echo = ping.echo();
    assert!(!KeepAlive::parse(&ping.to_bytes()).unwrap().is_echo);
    assert!(KeepAlive::parse(&echo.to_bytes()).unwrap().is_echo);
    assert_eq!(
        KeepAlive::parse(&echo.to_bytes()).unwrap().nonce,
        ping.nonce
    );
}
