#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use proptest::prelude::*;

use super::*;

fn header(body_len: usize) -> FrameHeader {
    FrameHeader::new(
        FrameKind::Chunk,
        LaneId::new(7),
        SequenceNumber::new(9),
        body_len,
    )
    .unwrap()
}

#[test]
fn header_round_trips() {
    let original = header(1024);
    let parsed = FrameHeader::parse(&original.to_bytes()).unwrap();
    assert_eq!(original, parsed);
}

#[test]
fn header_is_eight_bytes() {
    assert_eq!(header(0).to_bytes().len(), FrameHeader::LEN);
}

#[test]
fn zero_bytes_are_not_a_frame() {
    assert_eq!(
        FrameHeader::parse(&[0u8; FrameHeader::LEN]),
        Err(WireError::UnknownFrameKind(0))
    );
}

#[test]
fn truncated_header_is_rejected() {
    let encoded = header(64).to_bytes();
    for short in 0..FrameHeader::LEN {
        assert!(
            FrameHeader::parse(&encoded[..short]).is_err(),
            "accepted {short} bytes"
        );
    }
}

#[test]
fn body_beyond_the_length_field_is_rejected() {
    let too_long = MAX_BODY_LEN + 1;
    assert_eq!(
        FrameHeader::new(
            FrameKind::Chunk,
            LaneId::HANDSHAKE,
            SequenceNumber::ZERO,
            too_long
        ),
        Err(WireError::BodyTooLong(too_long))
    );
    assert!(
        FrameHeader::new(
            FrameKind::Chunk,
            LaneId::HANDSHAKE,
            SequenceNumber::ZERO,
            MAX_BODY_LEN
        )
        .is_ok()
    );
}

#[test]
fn a_one_mebibyte_chunk_costs_twenty_eight_bytes() {
    const PAYLOAD: usize = 1 << 20;
    let body = CHUNK_HEADER_LEN + PAYLOAD;
    let overhead = header(body).sealed_frame_len() - PAYLOAD;
    assert_eq!(overhead, FrameHeader::LEN + CHUNK_HEADER_LEN + TAG_LEN);
    assert_eq!(overhead, 28);
}

#[test]
fn chunk_frame_round_trips() {
    let index = ChunkIndex::new(4242);
    let payload = b"the quick brown fox";
    let mut body = chunk_header_bytes(index).to_vec();
    body.extend_from_slice(payload);

    let parsed = ChunkFrame::parse(&body).unwrap();
    assert_eq!(parsed.index(), index);
    assert_eq!(parsed.payload(), payload);
}

#[test]
fn chunk_frame_rejects_a_body_shorter_than_its_index() {
    for short in 0..CHUNK_HEADER_LEN {
        assert!(
            ChunkFrame::parse(&vec![0u8; short]).is_err(),
            "accepted {short} bytes"
        );
    }
}

#[test]
fn an_empty_chunk_payload_parses() {
    let body = chunk_header_bytes(ChunkIndex::ZERO);
    assert_eq!(ChunkFrame::parse(&body).unwrap().payload(), b"");
}

#[test]
fn lane_ids_never_wrap() {
    assert_eq!(LaneId::MAX.next(), None);
    assert_eq!(LaneId::HANDSHAKE.next(), Some(LaneId::new(1)));
}

#[test]
fn chunk_byte_offsets_do_not_overflow_at_the_top_of_the_index_space() {
    const CHUNK_LEN: u32 = 1 << 20;
    let last = ChunkIndex::new(u32::MAX);
    assert_eq!(
        last.byte_offset(CHUNK_LEN),
        u64::from(u32::MAX) * u64::from(CHUNK_LEN)
    );
}

proptest! {
    #[test]
    fn any_header_round_trips(
        kind_byte in 1u8..=6,
        lane in any::<u16>(),
        seq in any::<u16>(),
        body_len in 0usize..=MAX_BODY_LEN,
    ) {
        let kind = FrameKind::from_byte(kind_byte).unwrap();
        let original =
            FrameHeader::new(kind, LaneId::new(lane), SequenceNumber::new(seq), body_len).unwrap();
        prop_assert_eq!(FrameHeader::parse(&original.to_bytes()).unwrap(), original);
    }

    #[test]
    fn parsing_never_panics(source in prop::collection::vec(any::<u8>(), 0..64)) {
        let _ = FrameHeader::parse(&source);
        let _ = ChunkFrame::parse(&source);
    }

    #[test]
    fn any_chunk_frame_round_trips(
        index in any::<u32>(),
        payload in prop::collection::vec(any::<u8>(), 0..512),
    ) {
        let index = ChunkIndex::new(index);
        let mut body = chunk_header_bytes(index).to_vec();
        body.extend_from_slice(&payload);
        let parsed = ChunkFrame::parse(&body).unwrap();
        prop_assert_eq!(parsed.index(), index);
        prop_assert_eq!(parsed.payload(), &payload[..]);
    }
}
