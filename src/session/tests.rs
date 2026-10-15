#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use crate::crypto::{CipherSuite, Direction, SessionSecret};
use crate::device::{Capabilities, DeviceId};
use crate::medium::{LaneProfile, LaneRegistryError};
use crate::wire::{FrameHeader, FrameKind, LaneId};

fn session(direction: Direction) -> Session {
    Session::new(
        DeviceId::from_public_key(&[3; 32]),
        SessionSecret::from_bytes([0x5C; 32]),
        direction,
        Capabilities::AES_HARDWARE,
        Capabilities::AES_HARDWARE,
    )
}

fn ready(direction: Direction) -> Session {
    let mut session = session(direction);
    session.signal(SessionSignal::Linked).unwrap();
    session
}

#[test]
fn a_session_starts_out_linking_and_cannot_move_data() {
    let session = session(Direction::Initiator);
    assert_eq!(session.state(), SessionState::Linking);
    assert!(!session.state().can_move_data());
}

#[test]
fn losing_every_lane_mid_transfer_suspends_rather_than_fails() {
    let mut session = ready(Direction::Initiator);
    session.signal(SessionSignal::TransferStarted).unwrap();
    assert_eq!(
        session.signal(SessionSignal::AllLanesLost).unwrap(),
        SessionState::Suspended
    );
    assert!(!session.state().is_final());

    assert_eq!(
        session.signal(SessionSignal::LaneAvailable).unwrap(),
        SessionState::Ready
    );
    assert_eq!(
        session.signal(SessionSignal::TransferStarted).unwrap(),
        SessionState::Transferring
    );
}

#[test]
fn losing_every_lane_while_idle_is_not_a_suspension() {
    let mut session = ready(Direction::Initiator);
    assert_eq!(
        session.signal(SessionSignal::AllLanesLost).unwrap(),
        SessionState::Ready
    );
}

#[test]
fn a_closed_session_accepts_nothing_further() {
    let mut session = ready(Direction::Initiator);
    assert_eq!(
        session.signal(SessionSignal::Closed).unwrap(),
        SessionState::Closed
    );
    assert!(session.state().is_final());
    assert_eq!(
        session.signal(SessionSignal::LaneAvailable),
        Err(SessionError::IllegalTransition {
            state: SessionState::Closed,
            signal: SessionSignal::LaneAvailable,
        })
    );
}

#[test]
fn a_transfer_cannot_start_before_the_link_is_settled() {
    let mut session = session(Direction::Initiator);
    assert!(session.signal(SessionSignal::TransferStarted).is_err());
}

#[test]
fn the_two_peers_of_a_session_can_read_each_other() {
    let mut initiator = ready(Direction::Initiator);
    let mut responder = ready(Direction::Responder);

    let lane = initiator.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    assert_eq!(
        responder.register_lane(LaneProfile::WIFI_INFRA).unwrap(),
        lane
    );

    let plaintext = b"clipboard contents".to_vec();
    let mut body = plaintext.clone();
    let header = initiator.seal(lane, FrameKind::Control, &mut body).unwrap();
    let header = FrameHeader::parse(&header).unwrap();

    assert_eq!(responder.open(&header, &mut body).unwrap(), &plaintext[..]);
}

#[test]
fn a_peer_cannot_read_its_own_frame() {
    let mut initiator = ready(Direction::Initiator);
    let lane = initiator.register_lane(LaneProfile::WIFI_INFRA).unwrap();

    let mut body = b"payload".to_vec();
    let header =
        FrameHeader::parse(&initiator.seal(lane, FrameKind::Control, &mut body).unwrap()).unwrap();

    // Each direction has its own key, so the sender's inbound cipher must reject this.
    assert!(initiator.open(&header, &mut body).is_err());
}

#[test]
fn a_frame_for_an_unregistered_lane_is_refused() {
    let mut session = ready(Direction::Initiator);
    let mut body = b"payload".to_vec();
    let absent = LaneId::new(9);
    assert_eq!(
        session.seal(absent, FrameKind::Control, &mut body),
        Err(SessionError::UnknownLane { lane: absent })
    );
}

#[test]
fn a_released_lane_takes_its_keys_with_it_and_its_identifier_is_retired() {
    let mut session = ready(Direction::Initiator);
    let first = session.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    session.release_lane(first);

    let mut body = b"payload".to_vec();
    assert!(session.seal(first, FrameKind::Control, &mut body).is_err());

    let second = session.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    assert_ne!(second, first, "a retired lane identifier came back");
}

#[test]
fn a_session_will_not_seal_before_it_is_ready() {
    let mut session = session(Direction::Initiator);
    let lane = session.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    let mut body = b"payload".to_vec();
    assert_eq!(
        session.seal(lane, FrameKind::Control, &mut body),
        Err(SessionError::NotTransferable(SessionState::Linking))
    );
}

#[test]
fn a_session_with_no_lanes_has_no_control_lane() {
    let session = ready(Direction::Initiator);
    assert_eq!(
        session.control_lane(),
        Err(LaneRegistryError::NoControlLane.into())
    );
    assert!(session.schedulable_bulk_lanes().is_empty());
}

#[test]
fn usb_and_wifi_lanes_are_both_schedulable_but_bluetooth_only_carries_control() {
    let mut session = ready(Direction::Initiator);
    let wifi = session.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    let usb = session.register_lane(LaneProfile::USB_ACCESSORY).unwrap();
    let bluetooth = session.register_lane(LaneProfile::BLUETOOTH_LE).unwrap();

    let bulk = session.schedulable_bulk_lanes();
    assert_eq!(bulk.len(), 2);
    assert!(bulk.contains(&wifi));
    assert!(bulk.contains(&usb));
    assert!(!bulk.contains(&bluetooth));
    assert_eq!(session.control_lane().unwrap(), usb);
}

#[test]
fn both_peers_negotiate_the_same_suite() {
    let with = Capabilities::AES_HARDWARE;
    let without = Capabilities::empty();
    let make = |local, remote| {
        Session::new(
            DeviceId::from_public_key(&[1; 32]),
            SessionSecret::from_bytes([0; 32]),
            Direction::Initiator,
            local,
            remote,
        )
        .cipher_suite()
    };
    assert_eq!(make(with, with), CipherSuite::Aes256Gcm);
    assert_eq!(make(with, without), make(without, with));
    assert_eq!(make(with, without), CipherSuite::ChaCha20Poly1305);
}

#[test]
fn every_state_ignores_a_signal_it_cannot_use_rather_than_guessing() {
    use SessionSignal::{AllLanesLost, LaneAvailable, Linked, TransferFinished, TransferStarted};
    use SessionState::{Closed, Linking, Ready, Suspended, Transferring};

    assert_eq!(Linking.apply(Linked), Some(Ready));
    assert_eq!(Linking.apply(TransferStarted), None);
    assert_eq!(Linking.apply(LaneAvailable), None);
    assert_eq!(Ready.apply(Linked), None);
    assert_eq!(Ready.apply(TransferFinished), None);
    assert_eq!(Transferring.apply(TransferFinished), Some(Ready));
    assert_eq!(Transferring.apply(AllLanesLost), Some(Suspended));
    assert_eq!(Suspended.apply(TransferStarted), Some(Transferring));
    assert_eq!(Closed.apply(Linked), None);

    for signal in [
        Linked,
        LaneAvailable,
        AllLanesLost,
        TransferStarted,
        TransferFinished,
    ] {
        assert_eq!(
            Closed.apply(signal),
            None,
            "{signal:?} revived a closed session"
        );
    }
}

#[test]
fn a_verification_code_is_six_digits_and_follows_the_transcript() {
    use crate::link::VerificationCode;
    let code = VerificationCode::from_transcript(b"transcript");
    assert_eq!(format!("{code}").len(), 6);
    assert!(code.as_u32() < 1_000_000);
    assert_eq!(code, VerificationCode::from_transcript(b"transcript"));
    assert_ne!(code, VerificationCode::from_transcript(b"transcripu"));
}

/// The two ends bring mediums up in whatever order their hardware allows, so agreeing on lane
/// identifiers by both counting from one only works while those orders happen to match. When
/// they do not, each end seals under a key derived from a different identifier and every frame
/// fails to authenticate. The initiator numbers; the responder adopts.
#[test]
fn a_responder_adopting_out_of_order_still_reads_the_initiator() {
    let mut initiator = ready(Direction::Initiator);
    let mut responder = ready(Direction::Responder);

    let bluetooth = initiator
        .register_lane(LaneProfile::BLUETOOTH_CLASSIC)
        .unwrap();
    let wifi = initiator.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    assert_ne!(bluetooth, wifi);

    // The phone saw Wi-Fi resolve before the Bluetooth socket finished connecting.
    responder.adopt_lane(wifi, LaneProfile::WIFI_INFRA).unwrap();
    responder
        .adopt_lane(bluetooth, LaneProfile::BLUETOOTH_CLASSIC)
        .unwrap();

    for lane in [wifi, bluetooth] {
        let plaintext = b"striped payload".to_vec();
        let mut body = plaintext.clone();
        let header = initiator.seal(lane, FrameKind::Control, &mut body).unwrap();
        let header = FrameHeader::parse(&header).unwrap();
        assert_eq!(responder.open(&header, &mut body).unwrap(), &plaintext[..]);
    }
}

#[test]
fn an_identifier_the_allocator_has_passed_cannot_be_adopted() {
    let mut session = ready(Direction::Responder);
    let taken = session.register_lane(LaneProfile::WIFI_INFRA).unwrap();
    assert!(matches!(
        session.adopt_lane(taken, LaneProfile::WIFI_INFRA),
        Err(SessionError::Registry(
            LaneRegistryError::LaneAlreadyIssued { .. }
        ))
    ));
}

/// Adopting must move the local allocator past the identifier, or a later local allocation
/// hands out one that is already keyed.
#[test]
fn adopting_moves_the_allocator_past_the_identifier() {
    let mut session = ready(Direction::Responder);
    session
        .adopt_lane(LaneId::new(9), LaneProfile::WIFI_INFRA)
        .unwrap();
    let next = session.register_lane(LaneProfile::USB_ACCESSORY).unwrap();
    assert!(next > LaneId::new(9));
}
