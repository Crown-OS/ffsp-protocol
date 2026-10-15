use thiserror::Error;

use crate::device::DeviceId;
use crate::link::LinkRecord;

#[derive(Debug, Error)]
pub enum LinkStoreError {
    #[error("no link is stored for {0}")]
    NotLinked(DeviceId),

    #[error("the link store is unavailable: {0}")]
    Unavailable(String),

    #[error("the stored link for {device} could not be decoded")]
    Corrupt { device: DeviceId },
}

/// Persistence for links, supplied by the platform.
///
/// The backing store is a keyring on Linux and the Android Keystore on Android, neither of which
/// this crate should know about.
pub trait LinkStore: Send + Sync {
    fn load(&self, device: &DeviceId) -> Result<Option<LinkRecord>, LinkStoreError>;
    fn store(&self, record: &LinkRecord) -> Result<(), LinkStoreError>;
    fn remove(&self, device: &DeviceId) -> Result<(), LinkStoreError>;
    fn all(&self) -> Result<Vec<LinkRecord>, LinkStoreError>;
}
