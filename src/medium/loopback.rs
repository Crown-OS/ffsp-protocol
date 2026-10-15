use tokio::io::DuplexStream;

use crate::medium::{FramedStreamLane, Lane, LaneProfile};
use crate::wire::LaneId;

/// Two lanes joined back to back, for exercising the protocol with no hardware.
pub fn loopback_pair(
    id: LaneId,
    buffer_len: usize,
) -> (
    FramedStreamLane<DuplexStream>,
    FramedStreamLane<DuplexStream>,
) {
    let (near, far) = tokio::io::duplex(buffer_len);
    (
        FramedStreamLane::new(id, LaneProfile::LOOPBACK, near),
        FramedStreamLane::new(id, LaneProfile::LOOPBACK, far),
    )
}

/// Boxed for callers that hold heterogeneous lanes.
pub fn boxed_loopback_pair(id: LaneId, buffer_len: usize) -> (Box<dyn Lane>, Box<dyn Lane>) {
    let (near, far) = loopback_pair(id, buffer_len);
    (Box::new(near), Box::new(far))
}

/// A full-duplex pair, for exercising the engine with no hardware.
pub fn loopback_channel_pair(
    id: LaneId,
    buffer_len: usize,
) -> (crate::medium::LaneChannel, crate::medium::LaneChannel) {
    let (near, far) = tokio::io::duplex(buffer_len);
    (
        crate::medium::framed_channel(id, LaneProfile::LOOPBACK, near),
        crate::medium::framed_channel(id, LaneProfile::LOOPBACK, far),
    )
}
