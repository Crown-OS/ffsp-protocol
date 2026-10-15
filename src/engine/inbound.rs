use std::fs::File;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::control::{ChunkAck, ControlMessage, KeepAlive, LaneReport, TransferId};
use crate::crypto::LaneCipher;
use crate::device::DeviceId;
use crate::io::{PositionalWrite, sealed_frame_capacity};
use crate::medium::LaneReadEnd;
use crate::platform::Clock;
use crate::transfer::{CHUNK_LEN, ChunkVerifier};
use crate::wire::{ChunkFrame, ChunkIndex, FrameHeader, FrameKind, LaneId};

/// Where verified chunks go, for as long as a transfer is running.
///
/// A lane outlives any one transfer, so the reader cannot own this; it borrows it for each chunk
/// and does the verifying and the writing itself. Handing the payload to the engine instead
/// would copy a megabyte per chunk to save nothing.
#[derive(Debug)]
pub(crate) struct InboundSink {
    pub(crate) transfer: TransferId,
    pub(crate) verifier: Arc<ChunkVerifier>,
    pub(crate) destination: Arc<File>,
    pub(crate) started_micros: u64,
}

pub(crate) type SharedSink = Arc<Mutex<Option<Arc<InboundSink>>>>;

/// What a reader tells the engine.
#[derive(Debug)]
pub(crate) struct Inbound {
    pub(crate) device: DeviceId,
    pub(crate) lane: LaneId,
    pub(crate) kind: InboundKind,
}

/// Lane identifiers are allocated per session, so two peers can both be using lane 1.
#[derive(Debug)]
pub(crate) enum InboundKind {
    Chunk {
        transfer: TransferId,
        index: ChunkIndex,
        payload_bytes: u64,
        frame_bytes: u64,
        arrived_at_micros: u64,
    },
    Control(Box<ControlMessage>),
    /// A control body the engine does not interpret, for the platform to deal with.
    Platform(Vec<u8>),
    Ack(ChunkAck),
    Report(LaneReport),
    KeepAlive {
        keepalive: KeepAlive,
        at_micros: u64,
    },
    Closed {
        reason: Option<String>,
    },
}

/// The only task that opens on this lane, in this direction.
#[derive(Debug)]
pub(crate) struct LaneReader {
    device: DeviceId,
    lane: LaneId,
    read: LaneReadEnd,
    cipher: LaneCipher,
    clock: Arc<dyn Clock>,
    sink: SharedSink,
    events: mpsc::UnboundedSender<Inbound>,
    frame: Vec<u8>,
}

impl LaneReader {
    pub(crate) fn new(
        device: DeviceId,
        read: LaneReadEnd,
        cipher: LaneCipher,
        clock: Arc<dyn Clock>,
        sink: SharedSink,
        events: mpsc::UnboundedSender<Inbound>,
    ) -> Self {
        Self {
            device,
            lane: read.id(),
            read,
            cipher,
            clock,
            sink,
            events,
            // One frame's worth per lane, reused. The buffer pool is the sender's backpressure;
            // on this side the socket already is.
            frame: vec![0u8; sealed_frame_capacity(CHUNK_LEN)],
        }
    }

    pub(crate) async fn run(mut self) {
        let reason = loop {
            match self.read_one().await {
                Ok(true) => {}
                Ok(false) => break None,
                Err(reason) => break Some(reason),
            }
        };
        let _ = self.events.send(self.wrap(InboundKind::Closed { reason }));
    }

    async fn read_one(&mut self) -> Result<bool, String> {
        let len = match self.read.recv_frame(&mut self.frame).await {
            Ok(len) => len,
            Err(error) => return Err(error.to_string()),
        };
        let at_micros = self.clock.now_micros();
        // Borrowed field by field: the frame buffer and the sink are held at the same time.
        let Self {
            device,
            lane,
            cipher,
            sink,
            events,
            frame,
            ..
        } = self;
        let (device, lane) = (*device, *lane);

        let (header_bytes, rest) = frame
            .split_at_mut_checked(FrameHeader::LEN)
            .ok_or_else(|| "frame shorter than a header".to_owned())?;
        let header = FrameHeader::parse(header_bytes).map_err(|error| error.to_string())?;
        let sealed = rest
            .get_mut(..len.saturating_sub(FrameHeader::LEN))
            .ok_or_else(|| "frame shorter than its header claimed".to_owned())?;

        // An authentication failure leaves the two nonce counters one apart for good, so there
        // is nothing to recover to: the lane ends here.
        let body = cipher
            .open(&header, sealed)
            .map_err(|error| error.to_string())?;

        let kind = match header.kind() {
            FrameKind::Chunk => {
                return absorb_chunk(device, lane, sink, events, body, len as u64, at_micros)
                    .map(|()| true);
            }
            FrameKind::Control => match body.first() {
                Some(first) if ControlMessage::is_engine_opcode(*first) => InboundKind::Control(
                    Box::new(ControlMessage::parse(body).map_err(|error| error.to_string())?),
                ),
                _ => InboundKind::Platform(body.to_vec()),
            },
            FrameKind::ChunkAck => {
                InboundKind::Ack(ChunkAck::parse(body).map_err(|error| error.to_string())?)
            }
            FrameKind::LaneReport => {
                InboundKind::Report(LaneReport::parse(body).map_err(|error| error.to_string())?)
            }
            FrameKind::KeepAlive => InboundKind::KeepAlive {
                keepalive: KeepAlive::parse(body).map_err(|error| error.to_string())?,
                at_micros,
            },
            FrameKind::Handshake => return Ok(false),
        };
        Ok(events.send(Inbound { device, lane, kind }).is_ok())
    }

    const fn wrap(&self, kind: InboundKind) -> Inbound {
        Inbound {
            device: self.device,
            lane: self.lane,
            kind,
        }
    }
}

/// Verifies and writes a chunk here, where its bytes already are.
///
/// Handing the payload back to the engine to write would copy a megabyte per chunk to save
/// nothing: verification is order-independent and positional writes need no coordination.
#[allow(clippy::too_many_arguments)]
fn absorb_chunk(
    device: DeviceId,
    lane: LaneId,
    sink: &SharedSink,
    events: &mpsc::UnboundedSender<Inbound>,
    body: &[u8],
    frame_bytes: u64,
    arrived_at_micros: u64,
) -> Result<(), String> {
    let chunk = ChunkFrame::parse(body).map_err(|error| error.to_string())?;
    let Some(sink) = sink.lock().ok().and_then(|guard| guard.clone()) else {
        // No transfer is running, so this chunk belongs to nothing.
        return Ok(());
    };
    sink.verifier
        .verify(chunk.index(), chunk.payload())
        .map_err(|error| error.to_string())?;
    let offset = sink
        .verifier
        .layout()
        .byte_offset(chunk.index())
        .ok_or_else(|| "chunk index outside the payload".to_owned())?;
    sink.destination
        .write_all_at(offset, chunk.payload())
        .map_err(|error| error.to_string())?;

    let _ = events.send(Inbound {
        device,
        lane,
        kind: InboundKind::Chunk {
            transfer: sink.transfer,
            index: chunk.index(),
            payload_bytes: chunk.payload().len() as u64,
            frame_bytes,
            arrived_at_micros: arrived_at_micros.saturating_sub(sink.started_micros),
        },
    });
    Ok(())
}
