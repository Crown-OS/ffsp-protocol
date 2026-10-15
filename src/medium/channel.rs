use core::fmt;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::medium::{LaneError, LaneFuture, LaneProfile};
use crate::wire::{FrameHeader, LaneId};

/// The writing half of a lane.
pub trait LaneSender: Send + fmt::Debug {
    /// Sends one whole frame. Partial sends are the implementation's problem, not the caller's.
    ///
    /// Header and body stay separate because the body is a sealed
    /// [`ChunkBuffer`](crate::io::ChunkBuffer) that is already where it wants to be; joining them
    /// first would copy a megabyte per chunk, which at three lanes is more memory traffic than
    /// the encryption.
    fn send_frame<'a>(
        &'a mut self,
        header: &'a [u8; FrameHeader::LEN],
        body: &'a [u8],
    ) -> LaneFuture<'a, ()>;

    fn close(&mut self) -> LaneFuture<'_, ()>;
}

/// The reading half of a lane.
pub trait LaneReceiver: Send + fmt::Debug {
    /// Reads one whole frame into `into`, returning its length.
    fn recv_frame<'a>(&'a mut self, into: &'a mut [u8]) -> LaneFuture<'a, usize>;
}

/// One lane, with its two directions separable.
///
/// [`Lane`](crate::medium::Lane) takes `&mut self` to both send and receive, which is honest for a
/// single owner but makes a lane that must carry chunks one way and acknowledgements the other
/// impossible to drive from two tasks. Splitting the directions is what lets every lane be full
/// duplex, and so what lets round-trip time be measured on each medium separately instead of
/// inferred through whichever lane happens to carry control frames.
#[derive(Debug)]
pub struct LaneChannel {
    id: LaneId,
    profile: LaneProfile,
    sender: Box<dyn LaneSender>,
    receiver: Box<dyn LaneReceiver>,
}

/// The half a lane's writer task owns.
#[derive(Debug)]
pub struct LaneWriteEnd {
    id: LaneId,
    profile: LaneProfile,
    sender: Box<dyn LaneSender>,
}

/// The half a lane's reader task owns.
#[derive(Debug)]
pub struct LaneReadEnd {
    id: LaneId,
    profile: LaneProfile,
    receiver: Box<dyn LaneReceiver>,
}

impl LaneChannel {
    pub const fn new(
        id: LaneId,
        profile: LaneProfile,
        sender: Box<dyn LaneSender>,
        receiver: Box<dyn LaneReceiver>,
    ) -> Self {
        Self {
            id,
            profile,
            sender,
            receiver,
        }
    }

    pub const fn id(&self) -> LaneId {
        self.id
    }

    pub const fn profile(&self) -> &LaneProfile {
        &self.profile
    }

    /// Renumbers the lane once the session has decided what to call it.
    ///
    /// A driver has to name a lane to build one, but it cannot know the identifier: for the
    /// first lane that is settled by the handshake it is about to carry, and for later ones by
    /// the peer. Getting this wrong is quiet — every lane reports itself as the same one and the
    /// scheduler credits a single lane with all of the work.
    pub const fn relabel(&mut self, id: LaneId) {
        self.id = id;
    }

    /// Borrows both directions at once, for the handshake that happens before there are tasks.
    pub fn halves_mut(&mut self) -> (&mut dyn LaneSender, &mut dyn LaneReceiver) {
        (&mut *self.sender, &mut *self.receiver)
    }

    /// Hands each direction to a different task.
    ///
    /// Exactly one task must own the write end, because the cipher sealing frames for this lane
    /// counts nonces in the order frames reach the wire.
    pub fn into_halves(self) -> (LaneWriteEnd, LaneReadEnd) {
        (
            LaneWriteEnd {
                id: self.id,
                profile: self.profile,
                sender: self.sender,
            },
            LaneReadEnd {
                id: self.id,
                profile: self.profile,
                receiver: self.receiver,
            },
        )
    }
}

impl LaneWriteEnd {
    pub const fn id(&self) -> LaneId {
        self.id
    }

    pub const fn profile(&self) -> &LaneProfile {
        &self.profile
    }

    pub fn send_frame<'a>(
        &'a mut self,
        header: &'a [u8; FrameHeader::LEN],
        body: &'a [u8],
    ) -> LaneFuture<'a, ()> {
        self.sender.send_frame(header, body)
    }

    pub fn close(&mut self) -> LaneFuture<'_, ()> {
        self.sender.close()
    }
}

impl LaneReadEnd {
    pub const fn id(&self) -> LaneId {
        self.id
    }

    pub const fn profile(&self) -> &LaneProfile {
        &self.profile
    }

    pub fn recv_frame<'a>(&'a mut self, into: &'a mut [u8]) -> LaneFuture<'a, usize> {
        self.receiver.recv_frame(into)
    }
}

/// Writes whole frames to a byte sink, a transfer at a time.
#[derive(Debug)]
pub struct FramedSender<W> {
    profile: LaneProfile,
    sink: W,
}

impl<W> FramedSender<W> {
    pub const fn new(profile: LaneProfile, sink: W) -> Self {
        Self { profile, sink }
    }
}

impl<W> LaneSender for FramedSender<W>
where
    W: AsyncWrite + Unpin + Send + fmt::Debug,
{
    /// A frame longer than the bus accepts in one go is written in several, which is what lets a
    /// 1 MiB chunk cross a USB pipe that takes 16 KiB at a time.
    fn send_frame<'a>(
        &'a mut self,
        header: &'a [u8; FrameHeader::LEN],
        body: &'a [u8],
    ) -> LaneFuture<'a, ()> {
        Box::pin(async move {
            let len = FrameHeader::LEN.saturating_add(body.len());
            if len > self.profile.max_frame_len {
                return Err(LaneError::FrameTooLong {
                    len,
                    max: self.profile.max_frame_len,
                });
            }
            self.sink.write_all(header).await?;
            for transfer in body.chunks(self.profile.max_transfer_len) {
                self.sink.write_all(transfer).await?;
            }
            self.sink.flush().await?;
            Ok(())
        })
    }

    fn close(&mut self) -> LaneFuture<'_, ()> {
        Box::pin(async move {
            self.sink.shutdown().await?;
            Ok(())
        })
    }
}

/// Reads whole frames from a byte source.
#[derive(Debug)]
pub struct FramedReceiver<R> {
    profile: LaneProfile,
    source: R,
}

impl<R> FramedReceiver<R> {
    pub const fn new(profile: LaneProfile, source: R) -> Self {
        Self { profile, source }
    }
}

impl<R> LaneReceiver for FramedReceiver<R>
where
    R: AsyncRead + Unpin + Send + fmt::Debug,
{
    /// Reads the header first, because it says how much more there is; `read_exact` already
    /// gathers however many transfers the bus needed to deliver it.
    ///
    /// A frame too long for `into` leaves the stream part way through a frame, so
    /// [`LaneError::FrameTooLong`] from this direction is fatal to the lane: the caller must close
    /// it rather than call again. Draining instead would let a peer name any length it liked and
    /// have us read it.
    fn recv_frame<'a>(&'a mut self, into: &'a mut [u8]) -> LaneFuture<'a, usize> {
        Box::pin(async move {
            let capacity = into.len();
            let (header_slot, remainder) =
                into.split_at_mut_checked(FrameHeader::LEN)
                    .ok_or(LaneError::FrameTooLong {
                        len: FrameHeader::LEN,
                        max: capacity,
                    })?;
            self.source.read_exact(header_slot).await?;

            let header = FrameHeader::parse(header_slot)?;
            let frame_len = header.sealed_frame_len();
            if frame_len > self.profile.max_frame_len {
                return Err(LaneError::FrameTooLong {
                    len: frame_len,
                    max: self.profile.max_frame_len,
                });
            }
            let body = remainder.get_mut(..frame_len - FrameHeader::LEN).ok_or(
                LaneError::FrameTooLong {
                    len: frame_len,
                    max: capacity,
                },
            )?;
            self.source.read_exact(body).await?;
            Ok(frame_len)
        })
    }
}

/// Turns any duplex byte stream into a full-duplex [`LaneChannel`].
pub fn framed_channel<S>(id: LaneId, profile: LaneProfile, stream: S) -> LaneChannel
where
    S: AsyncRead + AsyncWrite + Send + fmt::Debug + 'static,
{
    let (source, sink) = tokio::io::split(stream);
    LaneChannel::new(
        id,
        profile,
        Box::new(FramedSender::new(profile, sink)),
        Box::new(FramedReceiver::new(profile, source)),
    )
}
