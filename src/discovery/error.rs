use thiserror::Error;

use crate::device::DeviceError;
use crate::medium::MediumError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DiscoveryError {
    #[error("advertisement service data must be {expected} bytes, got {actual}")]
    AdvertisementLength { expected: usize, actual: usize },

    #[error("advertisement declares protocol version {0}, which this build cannot read")]
    UnsupportedVersion(u8),

    #[error(transparent)]
    Device(#[from] DeviceError),

    #[error("the epoch counter is outside the representable range")]
    EpochOutOfRange,

    #[error("mDNS TXT record is missing the {0} key")]
    MissingTxtKey(&'static str),

    #[error("mDNS TXT value for {key} is malformed")]
    MalformedTxtValue { key: &'static str },
}

impl From<MediumError> for DiscoveryError {
    fn from(_: MediumError) -> Self {
        Self::EpochOutOfRange
    }
}
