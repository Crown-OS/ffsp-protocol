//! Who a device is, and what it can do.

mod capability;
mod device_class;
mod device_id;
mod error;

pub use capability::Capabilities;
pub use device_class::DeviceClass;
pub use device_id::DeviceId;
pub use error::DeviceError;
