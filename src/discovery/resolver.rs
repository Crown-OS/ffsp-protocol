use std::collections::HashMap;

use crate::device::DeviceId;
use crate::discovery::{EphemeralId, Epoch, ephemeral_id};
use crate::link::LinkRecord;

/// Turns an advertised identifier back into the peer that sent it.
///
/// Every candidate identifier for every link is precomputed once per epoch, so a scan result
/// resolves with a single hash lookup no matter how many devices are linked. A beacon from a
/// device that is not linked resolves to nothing and is indistinguishable from noise.
#[derive(Debug)]
pub struct PresenceResolver {
    epoch: Epoch,
    tolerance: u8,
    table: HashMap<EphemeralId, DeviceId>,
}

impl PresenceResolver {
    /// A tolerance of one covers the fifteen minutes of clock skew either side of the local clock.
    pub const DEFAULT_TOLERANCE: u8 = 1;

    /// Widened tolerance for a device whose clock is badly wrong, such as a laptop with a dead
    /// real-time clock immediately after boot.
    pub const SKEWED_TOLERANCE: u8 = 4;

    pub fn build(links: &[LinkRecord], epoch: Epoch, tolerance: u8) -> Self {
        let candidates = usize::from(tolerance) * 2 + 1;
        let mut table = HashMap::with_capacity(links.len() * candidates);
        for link in links {
            for candidate in epoch.window(tolerance) {
                table.insert(ephemeral_id(&link.secret, candidate), link.device);
            }
        }
        Self {
            epoch,
            tolerance,
            table,
        }
    }

    pub fn resolve(&self, advertised: &EphemeralId) -> Option<DeviceId> {
        self.table.get(advertised).copied()
    }

    pub const fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub const fn tolerance(&self) -> u8 {
        self.tolerance
    }

    pub const fn is_current_for(&self, now: Epoch) -> bool {
        self.epoch.as_u64() == now.as_u64()
    }

    /// Whether a beacon's epoch byte is one this resolver already tried.
    ///
    /// A beacon that fails to resolve *and* reports an epoch byte outside the window is evidence
    /// of clock skew rather than an unknown device, and is the signal to rebuild with
    /// [`SKEWED_TOLERANCE`](Self::SKEWED_TOLERANCE).
    pub fn covers_epoch_byte(&self, epoch_low: u8) -> bool {
        self.epoch
            .window(self.tolerance)
            .any(|epoch| epoch.low_byte() == epoch_low)
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}
