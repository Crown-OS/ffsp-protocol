use thiserror::Error;

use crate::device::DeviceError;
use crate::medium::MediumError;
use crate::transfer::TransferError;

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("control opcode {0} is not known to this protocol version")]
    UnknownOpcode(u8),

    #[error("end reason {0} is not known to this protocol version")]
    UnknownReason(u8),

    #[error("the control body ended after {read} bytes with more expected")]
    Truncated { read: usize },

    #[error("{trailing} bytes follow the end of the control body")]
    TrailingBytes { trailing: usize },

    #[error("a control field is longer than its length prefix can describe")]
    FieldTooLong,

    #[error(transparent)]
    Transfer(#[from] TransferError),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error(transparent)]
    Medium(#[from] MediumError),
}
