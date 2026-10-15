use crate::device::{Capabilities, DeviceClass, DeviceId};
use crate::link::LinkSecret;

/// A device this device has been linked with.
///
/// Linking happens once and outlives sessions, reboots and address changes; nearness is a separate
/// and much more volatile question answered by
/// [`PresenceTracker`](crate::discovery::PresenceTracker).
#[derive(Debug, Clone)]
pub struct LinkRecord {
    pub device: DeviceId,
    pub public_key: [u8; 32],
    pub name: String,
    pub class: DeviceClass,
    pub capabilities: Capabilities,
    pub secret: LinkSecret,
}

impl LinkRecord {
    pub fn new(
        public_key: [u8; 32],
        name: String,
        class: DeviceClass,
        capabilities: Capabilities,
        secret: LinkSecret,
    ) -> Self {
        Self {
            device: DeviceId::from_public_key(&public_key),
            public_key,
            name,
            class,
            capabilities,
            secret,
        }
    }
}
