//! The lane abstraction every transport reaches this crate through.

mod accessory;
mod channel;
mod driver;
mod error;
mod framed_stream;
mod kind;
mod lane;
mod lane_profile;
mod lane_registry;
mod loopback;

pub use accessory::{AOA_DESCRIPTION, AOA_MANUFACTURER, AOA_MODEL, AOA_URI, AOA_VERSION};
pub use channel::{
    FramedReceiver, FramedSender, LaneChannel, LaneReadEnd, LaneReceiver, LaneSender, LaneWriteEnd,
    framed_channel,
};
pub use driver::{DriverFuture, MediumDriver};
pub use error::{LaneError, LaneRegistryError, MediumError};
pub use framed_stream::FramedStreamLane;
pub use kind::Medium;
pub use lane::{Lane, LaneFuture};
pub use lane_profile::{
    AOA_MAX_TRANSFER, BULK_FRAME_CEILING, LaneFlags, LaneProfile, RFCOMM_MAX_TRANSFER, RadioGroup,
};
pub use lane_registry::{LaneEntry, LaneRegistry};
pub use loopback::{boxed_loopback_pair, loopback_channel_pair, loopback_pair};

#[cfg(test)]
mod tests;
