use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeviceError {
    #[error("device class {0} is not known to this protocol version")]
    UnknownDeviceClass(u8),
}
