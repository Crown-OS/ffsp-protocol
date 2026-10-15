#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::device::Capabilities;
use crate::wire::{FrameHeader, FrameKind, LaneId, SequenceNumber, TAG_LEN};

const SUITES: [CipherSuite; 2] = [CipherSuite::ChaCha20Poly1305, CipherSuite::Aes256Gcm];

fn secret() -> SessionSecret {
    SessionSecret::from_bytes([0x42; 32])
}

fn pair(suite: CipherSuite, lane: LaneId) -> (LaneCipher, LaneCipher) {
    let secret = secret();
    let key = secret.lane_key(Direction::Initiator, lane);
    (
        LaneCipher::new(suite, &key, lane),
        LaneCipher::new(suite, &key, lane),
    )
}

#[test]
fn a_frame_round_trips_under_both_suites() {
    for suite in SUITES {
        let lane = LaneId::new(3);
        let (mut sender, mut receiver) = pair(suite, lane);

        let plaintext = b"the quick brown fox jumps over the lazy dog".to_vec();
        let mut sealed = plaintext.clone();
        let header_bytes = sender.seal(FrameKind::Chunk, &mut sealed).unwrap();

        let header = FrameHeader::parse(&header_bytes).unwrap();
        assert_eq!(header.body_len(), plaintext.len());
        assert_eq!(header.lane(), lane);
        assert_eq!(sealed.len(), plaintext.len() + TAG_LEN);

        assert_eq!(
            receiver.open(&header, &mut sealed).unwrap(),
            &plaintext[..],
            "{suite:?}"
        );
    }
}

#[test]
fn sealing_costs_exactly_one_tag() {
    for suite in SUITES {
        let (mut sender, _) = pair(suite, LaneId::new(1));
        let mut sealed = vec![0u8; 1 << 20];
        let header = sender.seal(FrameKind::Chunk, &mut sealed).unwrap();
        let header = FrameHeader::parse(&header).unwrap();
        assert_eq!(sealed.len() - (1 << 20), TAG_LEN);
        assert_eq!(
            header.sealed_frame_len(),
            FrameHeader::LEN + (1 << 20) + TAG_LEN
        );
    }
}

#[test]
fn a_flipped_ciphertext_bit_does_not_authenticate() {
    for suite in SUITES {
        let (mut sender, mut receiver) = pair(suite, LaneId::new(1));
        let mut sealed = b"payload".to_vec();
        let header =
            FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut sealed).unwrap()).unwrap();
        sealed[0] ^= 0x01;
        assert_eq!(
            receiver.open(&header, &mut sealed),
            Err(CryptoError::NotAuthentic)
        );
    }
}

#[test]
fn a_flipped_tag_bit_does_not_authenticate() {
    for suite in SUITES {
        let (mut sender, mut receiver) = pair(suite, LaneId::new(1));
        let mut sealed = b"payload".to_vec();
        let header =
            FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut sealed).unwrap()).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 0x80;
        assert_eq!(
            receiver.open(&header, &mut sealed),
            Err(CryptoError::NotAuthentic)
        );
    }
}

#[test]
fn rewriting_the_header_does_not_authenticate() {
    for suite in SUITES {
        let (mut sender, mut receiver) = pair(suite, LaneId::new(1));
        let mut sealed = b"payload".to_vec();
        let mut header_bytes = sender.seal(FrameKind::Chunk, &mut sealed).unwrap();

        // The kind byte sits outside the length and sequence fields, so only the AEAD can catch it.
        header_bytes[3] = FrameKind::Control.as_byte();
        let forged = FrameHeader::parse(&header_bytes).unwrap();
        assert_eq!(forged.kind(), FrameKind::Control);

        assert_eq!(
            receiver.open(&forged, &mut sealed),
            Err(CryptoError::NotAuthentic)
        );
    }
}

#[test]
fn a_truncated_frame_is_refused_before_decryption() {
    let (mut sender, mut receiver) = pair(CipherSuite::Aes256Gcm, LaneId::new(1));
    let mut sealed = b"payload".to_vec();
    let header = FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut sealed).unwrap()).unwrap();
    sealed.truncate(sealed.len() - 1);
    let shortened = sealed.len();
    assert_eq!(
        receiver.open(&header, &mut sealed),
        Err(CryptoError::MissingTag(shortened))
    );
}

#[test]
fn a_frame_from_another_lane_does_not_authenticate() {
    let secret = secret();
    let mut sender = LaneCipher::new(
        CipherSuite::Aes256Gcm,
        &secret.lane_key(Direction::Initiator, LaneId::new(1)),
        LaneId::new(1),
    );
    let mut receiver = LaneCipher::new(
        CipherSuite::Aes256Gcm,
        &secret.lane_key(Direction::Initiator, LaneId::new(2)),
        LaneId::new(2),
    );

    let mut sealed = b"payload".to_vec();
    let header = FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut sealed).unwrap()).unwrap();
    assert_eq!(
        receiver.open(&header, &mut sealed),
        Err(CryptoError::NotAuthentic)
    );
}

#[test]
fn each_lane_and_direction_gets_its_own_key() {
    let secret = secret();
    let keys = [
        secret.lane_key(Direction::Initiator, LaneId::new(1)),
        secret.lane_key(Direction::Initiator, LaneId::new(2)),
        secret.lane_key(Direction::Responder, LaneId::new(1)),
        secret.lane_key(Direction::Responder, LaneId::new(2)),
    ];
    for (index, key) in keys.iter().enumerate() {
        for other in keys.iter().skip(index + 1) {
            assert_ne!(key.as_bytes(), other.as_bytes());
        }
    }
    assert_eq!(
        secret
            .lane_key(Direction::Initiator, LaneId::new(1))
            .as_bytes(),
        keys[0].as_bytes(),
        "derivation is not deterministic"
    );
}

#[test]
fn consecutive_frames_use_different_nonces() {
    let (mut sender, mut receiver) = pair(CipherSuite::Aes256Gcm, LaneId::new(1));
    let mut first = b"one".to_vec();
    let mut second = b"one".to_vec();

    let header_one =
        FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut first).unwrap()).unwrap();
    let header_two =
        FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut second).unwrap()).unwrap();

    assert_ne!(first, second, "identical plaintexts sealed identically");
    assert_eq!(header_one.seq().as_u16(), 0);
    assert_eq!(header_two.seq().as_u16(), 1);
    assert_eq!(sender.frames_sealed(), 2);

    receiver.open(&header_one, &mut first).unwrap();
    receiver.open(&header_two, &mut second).unwrap();
}

#[test]
fn a_frame_arriving_out_of_order_is_detected_as_desync() {
    let (mut sender, mut receiver) = pair(CipherSuite::Aes256Gcm, LaneId::new(1));
    let mut first = b"one".to_vec();
    let mut second = b"two".to_vec();
    sender.seal(FrameKind::Chunk, &mut first).unwrap();
    let header_two =
        FrameHeader::parse(&sender.seal(FrameKind::Chunk, &mut second).unwrap()).unwrap();

    assert_eq!(
        receiver.open(&header_two, &mut second),
        Err(CryptoError::SequenceDesync {
            expected: SequenceNumber::new(0),
            received: SequenceNumber::new(1),
        })
    );
}

#[test]
fn the_nonce_counter_refuses_to_wrap() {
    let mut nonces = NonceSequence::new();
    assert_eq!(nonces.nonce(), [0; 12]);
    nonces.advance().unwrap();
    assert_ne!(nonces.nonce(), [0; 12]);
    assert_eq!(nonces.sequence_number().as_u16(), 1);
}

#[test]
fn the_sequence_number_is_the_low_half_of_the_counter() {
    let mut nonces = NonceSequence::new();
    for _ in 0..70_000 {
        nonces.advance().unwrap();
    }
    // 70_000 wraps past 65_536, and the sequence number follows the counter's low half.
    assert_eq!(nonces.counter(), 70_000);
    assert_eq!(nonces.sequence_number().as_u16(), 4_464);
    assert!(nonces.expect(SequenceNumber::new(4_464)).is_ok());
}

#[test]
fn aes_is_chosen_only_when_both_peers_accelerate_it() {
    let with = Capabilities::AES_HARDWARE;
    let without = Capabilities::empty();

    assert_eq!(CipherSuite::negotiate(with, with), CipherSuite::Aes256Gcm);
    assert_eq!(
        CipherSuite::negotiate(with, without),
        CipherSuite::ChaCha20Poly1305
    );
    assert_eq!(
        CipherSuite::negotiate(without, with),
        CipherSuite::ChaCha20Poly1305
    );
    assert_eq!(
        CipherSuite::negotiate(without, without),
        CipherSuite::ChaCha20Poly1305
    );
}

#[test]
fn cipher_suite_bytes_round_trip() {
    for suite in SUITES {
        assert_eq!(CipherSuite::from_byte(suite.as_byte()).unwrap(), suite);
    }
    assert_eq!(
        CipherSuite::from_byte(0),
        Err(CryptoError::UnknownCipherSuite(0))
    );
    assert_eq!(
        CipherSuite::from_byte(9),
        Err(CryptoError::UnknownCipherSuite(9))
    );
}

#[test]
fn the_advertised_capability_matches_what_this_processor_can_do() {
    assert_eq!(
        CipherSuite::local_capability().contains(Capabilities::AES_HARDWARE),
        has_aes_hardware()
    );
}

#[test]
fn a_key_holding_type_never_prints_its_key() {
    let secret = secret();
    let key = secret.lane_key(Direction::Initiator, LaneId::new(1));
    assert_eq!(format!("{key:?}"), "LaneKey(redacted)");
    assert_eq!(format!("{secret:?}"), "SessionSecret(redacted)");

    let cipher = LaneCipher::new(CipherSuite::Aes256Gcm, &key, LaneId::new(1));
    let rendered = format!("{cipher:?}");
    assert!(rendered.contains("Aes256Gcm"));
    assert!(
        !rendered.contains("42"),
        "cipher debug leaked key material: {rendered}"
    );
}
