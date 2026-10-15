#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::crypto::Direction;

/// Exchanges messages until both sides are done, returning how many crossed the wire.
fn run(initiator: &mut Handshake, responder: &mut Handshake) -> usize {
    let mut buffer = [0u8; Handshake::MAX_MESSAGE_LEN];
    let mut messages = 0;
    while (!initiator.is_finished() || !responder.is_finished()) && messages < 8 {
        let (writer, reader) = if initiator.is_my_turn() {
            (&mut *initiator, &mut *responder)
        } else {
            (&mut *responder, &mut *initiator)
        };
        let len = writer.write_message(&mut buffer).unwrap();
        reader.read_message(&buffer[..len]).unwrap();
        messages += 1;
    }
    assert!(
        initiator.is_finished() && responder.is_finished(),
        "handshake stalled"
    );
    messages
}

fn identities() -> (StaticIdentity, StaticIdentity) {
    (generate_identity().unwrap(), generate_identity().unwrap())
}

#[test]
fn a_first_pairing_leaves_both_sides_holding_the_same_secrets() {
    let (phone, laptop) = identities();
    let mut initiator = Handshake::pair_initiator(&phone).unwrap();
    let mut responder = Handshake::pair_responder(&laptop).unwrap();

    assert_eq!(
        run(&mut initiator, &mut responder),
        3,
        "XX should take three messages"
    );
    assert_eq!(initiator.verification_code(), responder.verification_code());

    let from_phone = initiator.finish().unwrap();
    let from_laptop = responder.finish().unwrap();

    assert_eq!(from_phone.link_secret, from_laptop.link_secret);
    assert!(
        from_phone.link_secret.is_some(),
        "a first pairing must establish a link secret"
    );
    assert_eq!(from_phone.peer, laptop.device_id());
    assert_eq!(from_laptop.peer, phone.device_id());
    assert_eq!(from_phone.direction, Direction::Initiator);
    assert_eq!(from_laptop.direction, Direction::Responder);
}

#[test]
fn the_two_sides_of_a_pairing_can_talk_afterwards() {
    use crate::crypto::{CipherSuite, LaneCipher};
    use crate::wire::{FrameHeader, FrameKind, LaneId};

    let (phone, laptop) = identities();
    let mut initiator = Handshake::pair_initiator(&phone).unwrap();
    let mut responder = Handshake::pair_responder(&laptop).unwrap();
    run(&mut initiator, &mut responder);

    let phone_side = initiator.finish().unwrap();
    let laptop_side = responder.finish().unwrap();

    let lane = LaneId::new(1);
    let mut sender = LaneCipher::new(
        CipherSuite::Aes256Gcm,
        &phone_side
            .session_secret
            .lane_key(Direction::Initiator, lane),
        lane,
    );
    let mut receiver = LaneCipher::new(
        CipherSuite::Aes256Gcm,
        &laptop_side
            .session_secret
            .lane_key(Direction::Initiator, lane),
        lane,
    );

    let plaintext = b"derived independently on both sides".to_vec();
    let mut body = plaintext.clone();
    let header = FrameHeader::parse(&sender.seal(FrameKind::Control, &mut body).unwrap()).unwrap();
    assert_eq!(receiver.open(&header, &mut body).unwrap(), &plaintext[..]);
}

#[test]
fn reconnecting_to_a_linked_device_takes_one_round_trip() {
    let (phone, laptop) = identities();
    let mut initiator = Handshake::reconnect_initiator(&phone, laptop.public_key()).unwrap();
    let mut responder = Handshake::reconnect_responder(&laptop).unwrap();

    assert_eq!(
        run(&mut initiator, &mut responder),
        2,
        "IK should take two messages"
    );
    assert_eq!(initiator.verification_code(), responder.verification_code());

    let from_phone = initiator.finish().unwrap();
    let from_laptop = responder.finish().unwrap();
    assert_eq!(from_phone.peer, laptop.device_id());
    assert_eq!(from_laptop.peer, phone.device_id());
    assert!(
        from_phone.link_secret.is_none(),
        "a reconnect must not hand back a link secret to overwrite the stored one"
    );
    assert!(from_laptop.link_secret.is_none());
}

#[test]
fn every_session_derives_a_fresh_secret() {
    let (phone, laptop) = identities();

    let pair_once = || {
        let mut initiator = Handshake::reconnect_initiator(&phone, laptop.public_key()).unwrap();
        let mut responder = Handshake::reconnect_responder(&laptop).unwrap();
        run(&mut initiator, &mut responder);
        (initiator.finish().unwrap(), responder.finish().unwrap())
    };

    let (first, _) = pair_once();
    let (second, _) = pair_once();

    let lane = crate::wire::LaneId::new(1);
    assert_ne!(
        first
            .session_secret
            .lane_key(Direction::Initiator, lane)
            .as_bytes(),
        second
            .session_secret
            .lane_key(Direction::Initiator, lane)
            .as_bytes()
    );
}

#[test]
fn a_machine_in_the_middle_cannot_make_both_screens_agree() {
    let (phone, laptop) = identities();
    let attacker = generate_identity().unwrap();

    let mut phone_side = Handshake::pair_initiator(&phone).unwrap();
    let mut attacker_to_phone = Handshake::pair_responder(&attacker).unwrap();
    run(&mut phone_side, &mut attacker_to_phone);

    let mut attacker_to_laptop = Handshake::pair_initiator(&attacker).unwrap();
    let mut laptop_side = Handshake::pair_responder(&laptop).unwrap();
    run(&mut attacker_to_laptop, &mut laptop_side);

    assert_ne!(
        phone_side.verification_code(),
        laptop_side.verification_code(),
        "the interposed keys produced matching codes"
    );
    assert_eq!(phone_side.finish().unwrap().peer, attacker.device_id());
    assert_eq!(laptop_side.finish().unwrap().peer, attacker.device_id());
}

#[test]
fn reconnecting_to_the_wrong_key_fails() {
    let (phone, laptop) = identities();
    let stranger = generate_identity().unwrap();

    let mut initiator = Handshake::reconnect_initiator(&phone, stranger.public_key()).unwrap();
    let mut responder = Handshake::reconnect_responder(&laptop).unwrap();

    let mut buffer = [0u8; Handshake::MAX_MESSAGE_LEN];
    let len = initiator.write_message(&mut buffer).unwrap();
    assert!(
        responder.read_message(&buffer[..len]).is_err(),
        "accepted a foreign key"
    );
}

#[test]
fn an_unfinished_handshake_yields_no_keys() {
    let (phone, _) = identities();
    let initiator = Handshake::pair_initiator(&phone).unwrap();
    assert!(!initiator.is_finished());
    assert!(initiator.peer_public_key().is_none());
    assert!(matches!(initiator.finish(), Err(PairingError::Unfinished)));
}

#[test]
fn speaking_out_of_turn_is_refused() {
    let (phone, laptop) = identities();
    let mut initiator = Handshake::pair_initiator(&phone).unwrap();
    let mut responder = Handshake::pair_responder(&laptop).unwrap();
    let mut buffer = [0u8; Handshake::MAX_MESSAGE_LEN];

    assert!(!responder.is_my_turn());
    assert!(matches!(
        responder.write_message(&mut buffer),
        Err(PairingError::OutOfTurn)
    ));

    let len = initiator.write_message(&mut buffer).unwrap();
    responder.read_message(&buffer[..len]).unwrap();
    assert!(matches!(
        initiator.write_message(&mut buffer),
        Err(PairingError::OutOfTurn)
    ));
}

#[test]
fn a_tampered_handshake_message_is_rejected() {
    let (phone, laptop) = identities();
    let mut initiator = Handshake::pair_initiator(&phone).unwrap();
    let mut responder = Handshake::pair_responder(&laptop).unwrap();
    let mut buffer = [0u8; Handshake::MAX_MESSAGE_LEN];

    let len = initiator.write_message(&mut buffer).unwrap();
    responder.read_message(&buffer[..len]).unwrap();
    let len = responder.write_message(&mut buffer).unwrap();
    buffer[len / 2] ^= 0x01;
    assert!(matches!(
        initiator.read_message(&buffer[..len]),
        Err(PairingError::Rejected)
    ));
}

#[test]
fn generated_identities_are_distinct_and_name_themselves() {
    let first = generate_identity().unwrap();
    let second = generate_identity().unwrap();
    assert_ne!(first.public_key(), second.public_key());
    assert_ne!(first.device_id(), second.device_id());
    assert_eq!(first.device_id(), StaticIdentity::device_id(&first));
    assert_eq!(
        format!("{first:?}"),
        format!("StaticIdentity({})", first.device_id())
    );
}

#[test]
fn an_identity_never_prints_its_secret() {
    let identity = generate_identity().unwrap();
    let rendered = format!("{identity:?}");
    let secret_prefix = crate::util::hex::encode(&identity.secret_key()[..4]);
    assert!(
        !rendered.contains(&secret_prefix),
        "identity debug leaked key material"
    );
}

#[test]
fn a_link_record_derives_its_device_id_from_the_key_it_stores() {
    use crate::device::{Capabilities, DeviceClass, DeviceId};
    let identity = generate_identity().unwrap();
    let record = LinkRecord::new(
        *identity.public_key(),
        "Laptop".to_owned(),
        DeviceClass::Laptop,
        Capabilities::WIFI_INFRA,
        LinkSecret::from_bytes([1; 32]),
    );
    assert_eq!(
        record.device,
        DeviceId::from_public_key(identity.public_key())
    );
    assert_eq!(record.device, identity.device_id());
    assert_eq!(format!("{:?}", record.secret), "LinkSecret(redacted)");
}
