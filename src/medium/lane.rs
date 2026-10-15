use core::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::medium::{LaneError, LaneProfile};
use crate::wire::LaneId;

pub type LaneFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, LaneError>> + Send + 'a>>;

/// One reliable, ordered, frame-oriented pipe to a peer.
///
/// Frames rather than bytes is what lets a USB bulk pipe, a TCP stream and a Bluetooth socket
/// pumped through JNI all satisfy the same contract; a byte-stream interface would force the first
/// and last to pretend to something they cannot honestly provide. Stream transports reach this
/// contract through [`FramedStreamLane`](crate::medium::FramedStreamLane).
pub trait Lane: Send + fmt::Debug {
    fn id(&self) -> LaneId;

    fn profile(&self) -> &LaneProfile;

    /// Sends one whole frame. Partial sends are the implementation's problem, not the caller's.
    fn send_frame<'a>(&'a mut self, frame: &'a [u8]) -> LaneFuture<'a, ()>;

    /// Reads one whole frame into `into`, returning its length.
    fn recv_frame<'a>(&'a mut self, into: &'a mut [u8]) -> LaneFuture<'a, usize>;

    fn close(&mut self) -> LaneFuture<'_, ()>;
}
