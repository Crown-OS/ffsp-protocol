use crate::engine::EngineError;
use crate::link::{Handshake, HandshakeOutcome, StaticIdentity};
use crate::medium::LaneChannel;
use crate::wire::{FrameHeader, FrameKind, LaneId, SequenceNumber, TAG_LEN};

/// Handshake frames carry no tag, because there is not yet a key to make one with.
///
/// The frame length in the header still accounts for one, so the same reader can parse a frame
/// before and after the keys exist without being told which it is looking at. Zeroes stand in;
/// the handshake authenticates itself, and a peer that tampers with these bytes fails the
/// transcript check rather than a tag check.
const ABSENT_TAG: [u8; TAG_LEN] = [0u8; TAG_LEN];

/// Runs a Noise handshake over a lane that is not yet carrying anything else.
pub(crate) async fn perform(
    channel: &mut LaneChannel,
    identity: &StaticIdentity,
    initiate: bool,
    peer_public_key: Option<[u8; StaticIdentity::KEY_LEN]>,
) -> Result<HandshakeOutcome, EngineError> {
    let mut handshake = match (initiate, peer_public_key) {
        (true, Some(key)) => Handshake::reconnect_initiator(identity, &key)?,
        (true, None) => Handshake::pair_initiator(identity)?,
        (false, Some(_)) => Handshake::reconnect_responder(identity)?,
        (false, None) => Handshake::pair_responder(identity)?,
    };

    let mut scratch = [0u8; Handshake::MAX_MESSAGE_LEN];
    let mut frame = vec![0u8; FrameHeader::LEN + Handshake::MAX_MESSAGE_LEN + TAG_LEN];
    let mut sequence = SequenceNumber::ZERO;

    while !handshake.is_finished() {
        if handshake.is_my_turn() {
            let len = handshake.write_message(&mut scratch)?;
            let body = scratch.get(..len).ok_or(EngineError::HandshakeTooLong {
                len,
                max: Handshake::MAX_MESSAGE_LEN,
            })?;
            let header = FrameHeader::new(FrameKind::Handshake, LaneId::HANDSHAKE, sequence, len)?;
            let mut payload = Vec::with_capacity(len + TAG_LEN);
            payload.extend_from_slice(body);
            payload.extend_from_slice(&ABSENT_TAG);
            let (sender, _) = channel.halves_mut();
            sender.send_frame(&header.to_bytes(), &payload).await?;
            sequence = sequence.wrapping_next();
        } else {
            let (_, receiver) = channel.halves_mut();
            let len = receiver.recv_frame(&mut frame).await?;
            let header = FrameHeader::parse(
                frame
                    .get(..FrameHeader::LEN)
                    .ok_or(EngineError::HandshakeTooLong { len, max: len })?,
            )?;
            if header.kind() != FrameKind::Handshake {
                return Err(EngineError::UnexpectedFrame(header.kind()));
            }
            let body = frame
                .get(FrameHeader::LEN..FrameHeader::LEN + header.body_len())
                .ok_or(EngineError::HandshakeTooLong {
                    len: header.body_len(),
                    max: Handshake::MAX_MESSAGE_LEN,
                })?;
            handshake.read_message(body)?;
        }
    }

    let outcome = handshake.finish()?;
    Ok(outcome)
}

/// The identifier both ends give the lane the handshake ran on.
///
/// It cannot be negotiated, because negotiating it is what the first lane is for.
pub(crate) const FIRST_LANE: LaneId = LaneId::new(1);
