use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::medium::{Lane, LaneError, LaneFuture, LaneProfile};
use crate::wire::{FrameHeader, LaneId};

/// Turns any byte stream into a [`Lane`].
///
/// The eight-byte frame header already begins with the body length, so a stream transport needs no
/// framing of its own: read the header, and it says how much more to read.
#[derive(Debug)]
pub struct FramedStreamLane<S> {
    id: LaneId,
    profile: LaneProfile,
    stream: S,
}

impl<S> FramedStreamLane<S> {
    pub const fn new(id: LaneId, profile: LaneProfile, stream: S) -> Self {
        Self {
            id,
            profile,
            stream,
        }
    }
}

impl<S> Lane for FramedStreamLane<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + core::fmt::Debug,
{
    fn id(&self) -> LaneId {
        self.id
    }

    fn profile(&self) -> &LaneProfile {
        &self.profile
    }

    fn send_frame<'a>(&'a mut self, frame: &'a [u8]) -> LaneFuture<'a, ()> {
        Box::pin(async move {
            if frame.len() > self.profile.max_frame_len {
                return Err(LaneError::FrameTooLong {
                    len: frame.len(),
                    max: self.profile.max_frame_len,
                });
            }
            self.stream.write_all(frame).await?;
            self.stream.flush().await?;
            Ok(())
        })
    }

    /// Reads one whole frame, header included, so the caller has the associated data the AEAD
    /// needs alongside the ciphertext it protects.
    /// Reads one whole frame, header included, so the caller has the associated data the AEAD
    /// needs alongside the ciphertext it protects.
    fn recv_frame<'a>(&'a mut self, into: &'a mut [u8]) -> LaneFuture<'a, usize> {
        Box::pin(async move {
            let capacity = into.len();
            let (header_slot, remainder) =
                into.split_at_mut_checked(FrameHeader::LEN)
                    .ok_or(LaneError::FrameTooLong {
                        len: FrameHeader::LEN,
                        max: capacity,
                    })?;
            self.stream.read_exact(header_slot).await?;

            let header = FrameHeader::parse(header_slot)?;
            let frame_len = header.sealed_frame_len();
            let body = remainder.get_mut(..frame_len - FrameHeader::LEN).ok_or(
                LaneError::FrameTooLong {
                    len: frame_len,
                    max: capacity,
                },
            )?;
            self.stream.read_exact(body).await?;
            Ok(frame_len)
        })
    }

    fn close(&mut self) -> LaneFuture<'_, ()> {
        Box::pin(async move {
            self.stream.shutdown().await?;
            Ok(())
        })
    }
}
