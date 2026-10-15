#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::crypto::{CipherSuite, Direction, LaneCipher, SessionSecret};
use crate::engine::Tuning;
use crate::platform::PowerHint;
use crate::transfer::CHUNK_LEN;
use crate::wire::{FrameHeader, FrameKind, LaneId, TAG_LEN};

#[test]
fn a_chunk_buffer_is_page_aligned() {
    for capacity in [1024, 4096, sealed_chunk_capacity(CHUNK_LEN)] {
        let buffer = ChunkBuffer::with_capacity(capacity);
        assert!(
            buffer.is_page_aligned(),
            "capacity {capacity} produced an unaligned buffer"
        );
    }
}

#[test]
fn alignment_satisfies_both_page_sizes() {
    assert_eq!(PAGE_ALIGNMENT, 16 * 1024);
    assert!(
        PAGE_ALIGNMENT.is_multiple_of(4096),
        "not valid on a 4 KiB page"
    );
}

#[test]
fn a_chunk_buffer_fills_and_clears() {
    let mut buffer = ChunkBuffer::with_capacity(64);
    assert!(buffer.is_empty());
    buffer.fill_from(b"hello").unwrap();
    assert_eq!(buffer.as_slice(), b"hello");
    assert_eq!(buffer.len(), 5);
    buffer.clear();
    assert!(buffer.is_empty());
    assert_eq!(buffer.as_slice(), b"");
}

#[test]
fn a_chunk_buffer_refuses_more_than_it_can_hold() {
    let mut buffer = ChunkBuffer::with_capacity(4);
    assert!(buffer.fill_from(b"too long").is_err());
    assert!(buffer.commit(5).is_err());
    assert!(buffer.commit(4).is_ok());
    assert_eq!(buffer.len(), 4);
}

#[test]
fn spare_capacity_covers_exactly_what_is_left() {
    let mut buffer = ChunkBuffer::with_capacity(100);
    buffer.fill_from(&[1u8; 30]).unwrap();
    assert_eq!(buffer.spare_capacity_mut().len(), 70);
    buffer.spare_capacity_mut()[0] = 9;
    buffer.commit(1).unwrap();
    assert_eq!(buffer.len(), 31);
    assert_eq!(buffer.as_slice()[30], 9);
}

#[test]
fn a_sealed_chunk_fits_the_capacity_the_pool_hands_out() {
    let capacity = sealed_chunk_capacity(CHUNK_LEN);
    assert_eq!(capacity, CHUNK_LEN as usize + 4 + TAG_LEN);

    let secret = SessionSecret::from_bytes([7; 32]);
    let lane = LaneId::new(1);
    let key = secret.lane_key(Direction::Initiator, lane);
    let mut cipher = LaneCipher::new(CipherSuite::Aes256Gcm, &key, lane);

    let mut buffer = ChunkBuffer::with_capacity(capacity);
    buffer
        .fill_from(&vec![0xAB; CHUNK_LEN as usize + 4])
        .unwrap();
    let header = cipher.seal(FrameKind::Chunk, &mut buffer).unwrap();

    assert_eq!(
        buffer.len(),
        capacity,
        "the tag did not fit alongside the payload"
    );
    let header = FrameHeader::parse(&header).unwrap();
    assert_eq!(header.body_len(), CHUNK_LEN as usize + 4);
}

#[test]
fn a_chunk_seals_and_opens_inside_one_aligned_buffer() {
    let secret = SessionSecret::from_bytes([7; 32]);
    let lane = LaneId::new(1);
    let key = secret.lane_key(Direction::Initiator, lane);
    let mut sender = LaneCipher::new(CipherSuite::ChaCha20Poly1305, &key, lane);
    let mut receiver = LaneCipher::new(CipherSuite::ChaCha20Poly1305, &key, lane);

    let plaintext = vec![0x5A; 8192];
    let mut buffer = ChunkBuffer::with_capacity(plaintext.len() + TAG_LEN);
    buffer.fill_from(&plaintext).unwrap();
    assert!(buffer.is_page_aligned());

    let header = FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut buffer).unwrap()).unwrap();
    assert_ne!(buffer.as_slice()[..8], plaintext[..8]);

    let opened = receiver.open(&header, buffer.as_mut_slice()).unwrap();
    assert_eq!(opened, &plaintext[..]);
}

#[test]
fn an_empty_pool_is_the_backpressure_signal() {
    let pool = ChunkBufferPool::new(2, 128);
    assert_eq!(pool.idle(), 2);

    let first = pool.take().unwrap();
    let second = pool.take().unwrap();
    assert_eq!(pool.idle(), 0);
    assert!(
        pool.take().is_none(),
        "the pool handed out more than it holds"
    );

    pool.release(first);
    assert_eq!(pool.idle(), 1);
    pool.release(second);
    assert_eq!(pool.idle(), 2);
}

#[test]
fn a_recycled_buffer_carries_nothing_from_its_last_use() {
    let pool = ChunkBufferPool::new(1, 128);
    let mut buffer = pool.take().unwrap();
    buffer.fill_from(b"secret").unwrap();
    pool.release(buffer);

    let reused = pool.take().unwrap();
    assert!(reused.is_empty());
    assert_eq!(reused.as_slice(), b"");
}

#[test]
fn a_pool_of_thirty_two_one_mebibyte_buffers_bounds_memory() {
    let pool = ChunkBufferPool::new(32, sealed_chunk_capacity(CHUNK_LEN));
    assert_eq!(pool.idle(), 32);
    let footprint = 32 * pool.buffer_capacity();
    assert!(
        footprint < 64 * 1024 * 1024,
        "pool footprint is {footprint} bytes"
    );
}

#[test]
fn chunks_written_out_of_order_reconstruct_the_payload() {
    let directory = std::env::temp_dir().join(format!("ffsp-io-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("out-of-order.bin");

    let payload: Vec<u8> = (0..4096u32).map(|byte| (byte % 251) as u8).collect();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();

    assert!(file.supports_positional_write());
    reserve(&file, payload.len() as u64).unwrap();

    // Deliberately reversed: positional writes make arrival order irrelevant.
    for offset in (0..payload.len()).step_by(512).rev() {
        file.write_all_at(offset as u64, &payload[offset..offset + 512])
            .unwrap();
    }

    let mut readback = vec![0u8; payload.len()];
    file.read_exact_at(0, &mut readback).unwrap();
    assert_eq!(readback, payload);

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_pipe_is_reported_as_unsuitable_for_positional_writes() {
    let file = std::fs::File::open("/dev/null").unwrap();
    assert!(
        !file.supports_positional_write(),
        "a character device claimed pwrite support"
    );
}

/// A pool sized for a sealed chunk is eight bytes short of the frame that carries it, and the
/// shortfall only shows up on the receive path.
#[test]
fn a_pool_buffer_holds_a_whole_received_frame() {
    let capacity = sealed_frame_capacity(CHUNK_LEN);
    assert_eq!(
        capacity,
        FrameHeader::LEN + sealed_chunk_capacity(CHUNK_LEN)
    );
    let pool = ChunkBufferPool::new(1, capacity);
    let buffer = pool.take().unwrap();
    assert!(buffer.capacity() >= FrameHeader::LEN + sealed_chunk_capacity(CHUNK_LEN));
}

#[test]
fn the_reported_ceiling_counts_the_alignment_padding_that_is_really_allocated() {
    assert_eq!(buffer_footprint(1024), 1024 + PAGE_ALIGNMENT);
    let tuning = Tuning::for_power(PowerHint::Unconstrained);
    assert_eq!(
        tuning.buffer_footprint(),
        tuning.buffer_pool_size * buffer_footprint(tuning.buffer_capacity())
    );
}

#[test]
fn an_empty_pool_is_recorded_as_starvation_rather_than_passed_over() {
    let pool = ChunkBufferPool::new(1, 64);
    let held = pool.take().unwrap();
    assert_eq!(pool.starvations(), 0);
    assert!(pool.take().is_none());
    assert_eq!(pool.starvations(), 1);
    pool.release(held);
    assert!(pool.take().is_some());
}

/// Backpressure a caller can only poll leaves it choosing between spinning and sleeping past the
/// moment a buffer came free.
#[tokio::test]
async fn a_waiting_caller_is_woken_when_a_buffer_comes_back() {
    let pool = std::sync::Arc::new(ChunkBufferPool::new(1, 64));
    let held = pool.take().unwrap();

    let waiter = tokio::spawn({
        let pool = std::sync::Arc::clone(&pool);
        async move { pool.acquire().await.map(|buffer| buffer.capacity()) }
    });
    tokio::task::yield_now().await;
    pool.release(held);

    assert_eq!(waiter.await.unwrap(), Some(64));
    assert_eq!(pool.starvations(), 1);
}
