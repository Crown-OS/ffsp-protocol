use std::collections::BTreeSet;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::crypto::LaneCipher;
use crate::device::DeviceId;
use crate::io::{ChunkBuffer, ChunkBufferPool};
use crate::medium::LaneWriteEnd;
use crate::platform::Clock;
use crate::wire::{ChunkIndex, FrameKind, LaneId};

/// Work handed to a lane's writer.
#[derive(Debug)]
pub(crate) enum Outbound {
    /// A sealed-in-place chunk body: its index, its payload, and room for the tag.
    Chunk {
        index: ChunkIndex,
        buffer: ChunkBuffer,
    },
    Frame {
        kind: FrameKind,
        body: Vec<u8>,
    },
    /// Another lane delivered this chunk first; drop it if it has not gone out yet.
    Abandon(ChunkIndex),
    Close,
}

/// What a writer tells the engine after the fact.
#[derive(Debug)]
pub(crate) enum LaneNotice {
    Wrote {
        device: DeviceId,
        lane: LaneId,
        frame_bytes: u64,
        elapsed_micros: u64,
        chunk: Option<ChunkIndex>,
    },
    /// A chunk was dropped before it went out, so its buffer is free and the scheduler's
    /// in-flight count is wrong until it hears.
    Abandoned {
        device: DeviceId,
        lane: LaneId,
        index: ChunkIndex,
    },
    Failed {
        device: DeviceId,
        lane: LaneId,
        reason: String,
    },
}

/// The only task that seals on this lane, in this direction.
///
/// Everything with something to say on a lane queues it here unsealed, because the nonce counter
/// advances at the moment of sealing and the peer opens in the order frames arrive. Sealing
/// anywhere else would let those two orders differ.
#[derive(Debug)]
pub(crate) struct LaneWriter {
    device: DeviceId,
    lane: LaneId,
    write: LaneWriteEnd,
    cipher: LaneCipher,
    pool: Arc<ChunkBufferPool>,
    clock: Arc<dyn Clock>,
    notices: mpsc::UnboundedSender<LaneNotice>,
    abandoned: BTreeSet<ChunkIndex>,
}

impl LaneWriter {
    pub(crate) fn new(
        device: DeviceId,
        write: LaneWriteEnd,
        cipher: LaneCipher,
        pool: Arc<ChunkBufferPool>,
        clock: Arc<dyn Clock>,
        notices: mpsc::UnboundedSender<LaneNotice>,
    ) -> Self {
        Self {
            device,
            lane: write.id(),
            write,
            cipher,
            pool,
            clock,
            notices,
            abandoned: BTreeSet::new(),
        }
    }

    pub(crate) async fn run(
        mut self,
        mut bulk: mpsc::UnboundedReceiver<Outbound>,
        mut priority: mpsc::UnboundedReceiver<Outbound>,
    ) {
        loop {
            // Control frames overtake bulk work that has not started, so an acknowledgement is
            // not stuck behind a megabyte that Bluetooth will spend five seconds on. They cannot
            // overtake a frame already going out: that would desynchronise the framing.
            let item = tokio::select! {
                biased;
                Some(item) = priority.recv() => item,
                Some(item) = bulk.recv() => item,
                else => break,
            };
            match item {
                Outbound::Close => break,
                Outbound::Abandon(index) => {
                    self.abandoned.insert(index);
                }
                Outbound::Frame { kind, mut body } => {
                    if !self.write_frame(kind, &mut body, None).await {
                        break;
                    }
                }
                Outbound::Chunk { index, mut buffer } => {
                    if self.abandoned.remove(&index) {
                        self.pool.release(buffer);
                        let _ = self.notices.send(LaneNotice::Abandoned {
                            device: self.device,
                            lane: self.lane,
                            index,
                        });
                        continue;
                    }
                    let sent = self
                        .write_frame(FrameKind::Chunk, &mut buffer, Some(index))
                        .await;
                    self.pool.release(buffer);
                    if !sent {
                        break;
                    }
                }
            }
        }
        let _ = self.write.close().await;
    }

    async fn write_frame(
        &mut self,
        kind: FrameKind,
        body: &mut impl aes_gcm::aead::Buffer,
        chunk: Option<ChunkIndex>,
    ) -> bool {
        let header = match self.cipher.seal(kind, body) {
            Ok(header) => header,
            Err(error) => return self.fail(error.to_string()),
        };
        let began = self.clock.now_micros();
        if let Err(error) = self.write.send_frame(&header, body.as_ref()).await {
            return self.fail(error.to_string());
        }
        let elapsed_micros = self.clock.now_micros().saturating_sub(began);
        let frame_bytes = (crate::wire::FrameHeader::LEN + body.as_ref().len()) as u64;
        self.notices
            .send(LaneNotice::Wrote {
                device: self.device,
                lane: self.lane,
                frame_bytes,
                elapsed_micros,
                chunk,
            })
            .is_ok()
    }

    fn fail(&self, reason: String) -> bool {
        let _ = self.notices.send(LaneNotice::Failed {
            device: self.device,
            lane: self.lane,
            reason,
        });
        false
    }
}
