use core::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::device::DeviceId;
use crate::medium::{LaneChannel, LaneProfile, Medium, MediumError};
use crate::wire::LaneId;

pub type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, MediumError>> + Send + 'a>>;

/// Opens lanes over one medium.
///
/// Implementations live in the platform crates, because every one of them is a wrapper around an
/// operating system API: BlueZ and `nusb` on Linux, `UsbAccessory` and `BluetoothSocket` on
/// Android. This crate only decides what to do with the lanes they produce.
pub trait MediumDriver: Send + Sync + fmt::Debug {
    fn medium(&self) -> Medium;

    /// Whether the hardware and permissions are present right now. A driver that reports `false`
    /// is skipped rather than treated as a failure, so a device without Bluetooth is normal.
    fn is_available(&self) -> bool;

    fn profile(&self) -> LaneProfile {
        LaneProfile::of(self.medium())
    }

    /// Opens a lane, both directions, ready to be split between a writer and a reader task.
    fn open<'a>(&'a self, peer: &'a DeviceId, lane: LaneId) -> DriverFuture<'a, LaneChannel>;
}
