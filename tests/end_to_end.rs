//! Drives the whole stack over loopback lanes: schedule, seal, frame, open, verify, write.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::collections::BTreeMap;
use std::sync::Arc;

use ffsp_protocol::crypto::{CipherSuite, Direction, LaneCipher, SessionSecret};
use ffsp_protocol::io::{PositionalRead, PositionalWrite, reserve, sealed_chunk_capacity};
use ffsp_protocol::medium::{Lane, LaneProfile, LaneRegistry, loopback_pair};
use ffsp_protocol::transfer::{
    CHUNK_LEN, ChunkLayout, ChunkScheduler, ChunkVerifier, chaining_values_of,
};
use ffsp_protocol::wire::{ChunkFrame, FrameHeader, FrameKind, LaneId, chunk_header_bytes};

const MIB: usize = 1 << 20;

fn payload_of(len: usize) -> Vec<u8> {
    let mut state = 0x853C_49E6_748F_EA9Bu64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 41) as u8
        })
        .collect()
}

fn scratch_file(name: &str) -> (std::path::PathBuf, std::fs::File) {
    let directory = std::env::temp_dir().join(format!("ffsp-e2e-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("received.bin");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    (directory, file)
}

/// Moves `payload` across `lane_count` lanes and returns what the receiver wrote.
async fn transfer_over(lane_count: usize, payload: &[u8], name: &str) -> Vec<u8> {
    let layout = ChunkLayout::new(payload.len() as u64).unwrap();
    let root = blake3::hash(payload);
    let verifier = Arc::new(
        ChunkVerifier::new(root, layout, chaining_values_of(payload, &layout).unwrap()).unwrap(),
    );

    let secret = SessionSecret::from_bytes([0x9F; 32]);
    let suite = CipherSuite::negotiate(
        CipherSuite::local_capability(),
        CipherSuite::local_capability(),
    );

    let (directory, file) = scratch_file(name);
    reserve(&file, payload.len() as u64).unwrap();
    assert!(file.supports_positional_write());
    let destination = Arc::new(file);

    let mut registry = LaneRegistry::new();
    let mut scheduler = ChunkScheduler::new(layout);
    let mut senders: BTreeMap<LaneId, (_, LaneCipher)> = BTreeMap::new();
    let (delivered_tx, mut delivered_rx) = tokio::sync::mpsc::unbounded_channel();

    for _ in 0..lane_count {
        let id = registry.allocate(LaneProfile::LOOPBACK).unwrap();
        scheduler.add_lane(id, &LaneProfile::LOOPBACK);

        let (near, mut far) = loopback_pair(id, 8 * MIB);
        let key = secret.lane_key(Direction::Initiator, id);
        senders.insert(id, (near, LaneCipher::new(suite, &key, id)));

        let mut receiving_cipher = LaneCipher::new(suite, &key, id);
        let verifier = Arc::clone(&verifier);
        let destination = Arc::clone(&destination);
        let delivered = delivered_tx.clone();

        tokio::spawn(async move {
            let mut frame = vec![0u8; FrameHeader::LEN + sealed_chunk_capacity(CHUNK_LEN)];
            while let Ok(len) = far.recv_frame(&mut frame).await {
                let (header_bytes, sealed) = frame[..len].split_at(FrameHeader::LEN);
                let header = FrameHeader::parse(header_bytes).unwrap();
                let mut sealed = sealed.to_vec();

                let body = receiving_cipher.open(&header, &mut sealed).unwrap();
                let chunk = ChunkFrame::parse(body).unwrap();
                verifier.verify(chunk.index(), chunk.payload()).unwrap();

                let offset = verifier.layout().byte_offset(chunk.index()).unwrap();
                destination.write_all_at(offset, chunk.payload()).unwrap();

                if delivered.send((chunk.index(), header.lane())).is_err() {
                    return;
                }
            }
        });
    }
    drop(delivered_tx);

    let mut clock = 0u64;
    while !scheduler.is_complete() {
        while let Some(assignment) = scheduler.next_assignment(clock) {
            if assignment.duplicate {
                break;
            }
            let offset = layout.byte_offset(assignment.index).unwrap() as usize;
            let slice = &payload[offset..offset + assignment.len as usize];

            let Some((lane, cipher)) = senders.get_mut(&assignment.lane) else {
                continue;
            };
            let mut body = chunk_header_bytes(assignment.index).to_vec();
            body.extend_from_slice(slice);

            let header = cipher.seal(FrameKind::Chunk, &mut body).unwrap();
            let mut frame = header.to_vec();
            frame.extend_from_slice(&body);
            lane.send_frame(&frame).await.unwrap();
        }

        let Some((index, lane)) = delivered_rx.recv().await else {
            break;
        };
        clock += 1;
        scheduler.on_delivered(index, lane, clock);
    }

    assert!(scheduler.is_complete(), "transfer did not finish");
    assert_eq!(scheduler.progress().delivered_bytes, payload.len() as u64);

    let mut readback = vec![0u8; payload.len()];
    destination.read_exact_at(0, &mut readback).unwrap();
    let _ = std::fs::remove_dir_all(&directory);
    readback
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_payload_crosses_one_lane_and_reassembles_byte_for_byte() {
    let payload = payload_of(8 * MIB + 12_345);
    assert_eq!(transfer_over(1, &payload, "single").await, payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_payload_striped_over_three_lanes_reassembles_byte_for_byte() {
    let payload = payload_of(32 * MIB + 7);
    assert_eq!(transfer_over(3, &payload, "striped").await, payload);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_payload_smaller_than_one_chunk_still_crosses() {
    let payload = payload_of(4_096);
    assert_eq!(transfer_over(2, &payload, "tiny").await, payload);
}

#[tokio::test]
async fn a_frame_larger_than_the_lane_limit_is_refused_rather_than_split() {
    let (mut near, _far) = loopback_pair(LaneId::new(1), 1024);
    let oversized = vec![0u8; LaneProfile::LOOPBACK.max_frame_len + 1];
    assert!(near.send_frame(&oversized).await.is_err());
}

#[tokio::test]
async fn a_lane_reads_back_exactly_the_frame_that_was_written() {
    let (mut near, mut far) = loopback_pair(LaneId::new(4), 64 * 1024);

    let secret = SessionSecret::from_bytes([1; 32]);
    let key = secret.lane_key(Direction::Initiator, LaneId::new(4));
    let mut cipher = LaneCipher::new(CipherSuite::Aes256Gcm, &key, LaneId::new(4));

    let mut body = b"a short control message".to_vec();
    let header = cipher.seal(FrameKind::Control, &mut body).unwrap();
    let mut frame = header.to_vec();
    frame.extend_from_slice(&body);
    let expected = frame.clone();

    near.send_frame(&frame).await.unwrap();

    let mut received = vec![0u8; 4096];
    let len = far.recv_frame(&mut received).await.unwrap();
    assert_eq!(&received[..len], &expected[..]);
}
