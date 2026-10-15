use std::collections::{BTreeMap, BTreeSet};

use crate::medium::{LaneProfile, LaneRegistryError, RadioGroup};
use crate::wire::LaneId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneEntry {
    pub profile: LaneProfile,
    pub rtt_micros: u32,
    pub bandwidth_kbps: u32,
    /// Set when the marginal-contribution probe found the lane makes the transfer slower.
    pub parked: bool,
}

impl LaneEntry {
    const fn new(profile: LaneProfile) -> Self {
        Self {
            rtt_micros: profile.rtt_hint_micros,
            bandwidth_kbps: profile.bandwidth_hint_kbps,
            profile,
            parked: false,
        }
    }
}

/// Tracks the lanes of one session and allocates their identifiers.
///
/// This is the only source of [`LaneId`]s, and it never reissues one. A reissued id would pair a
/// fresh nonce counter with a key that has already used those nonces, so the session is ended
/// instead of wrapping.
#[derive(Debug)]
pub struct LaneRegistry {
    next: Option<LaneId>,
    entries: BTreeMap<LaneId, LaneEntry>,
    /// Every identifier this session has ever admitted, live or since released. Membership is
    /// what stops a released identifier being handed a fresh nonce counter under its old key.
    issued: BTreeSet<LaneId>,
}

impl Default for LaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LaneRegistry {
    pub const fn new() -> Self {
        Self {
            next: Some(LaneId::new(1)),
            entries: BTreeMap::new(),
            issued: BTreeSet::new(),
        }
    }

    pub fn allocate(&mut self, profile: LaneProfile) -> Result<LaneId, LaneRegistryError> {
        let mut id = self.next.ok_or(LaneRegistryError::LaneIdsExhausted)?;
        while self.issued.contains(&id) {
            id = id.next().ok_or(LaneRegistryError::LaneIdsExhausted)?;
        }
        self.next = id.next();
        self.admit(id, profile);
        Ok(id)
    }

    /// Registers a lane at an identifier the peer chose.
    ///
    /// Only the session initiator allocates. The two ends bring mediums up in whatever order
    /// their hardware allows — USB when the cable is plugged, Wi-Fi when discovery resolves — so
    /// numbering independently would have them agree only when those orders happened to match,
    /// and disagree by sealing under a key derived from a different identifier.
    pub fn adopt(&mut self, lane: LaneId, profile: LaneProfile) -> Result<(), LaneRegistryError> {
        if self.issued.contains(&lane) {
            return Err(LaneRegistryError::LaneAlreadyIssued { lane });
        }
        self.admit(lane, profile);
        // Keep the local allocator clear of anything adopted, whichever order they arrive in.
        if self.next.is_some_and(|next| lane >= next) {
            self.next = lane.next();
        }
        Ok(())
    }

    fn admit(&mut self, lane: LaneId, profile: LaneProfile) {
        self.entries.insert(lane, LaneEntry::new(profile));
        self.issued.insert(lane);
    }

    pub fn remove(&mut self, lane: LaneId) -> Option<LaneEntry> {
        self.entries.remove(&lane)
    }

    pub fn get(&self, lane: LaneId) -> Option<&LaneEntry> {
        self.entries.get(&lane)
    }

    pub fn observe(&mut self, lane: LaneId, rtt_micros: u32, bandwidth_kbps: u32) {
        if let Some(entry) = self.entries.get_mut(&lane) {
            entry.rtt_micros = rtt_micros;
            entry.bandwidth_kbps = bandwidth_kbps;
        }
    }

    pub fn set_parked(&mut self, lane: LaneId, parked: bool) {
        if let Some(entry) = self.entries.get_mut(&lane) {
            entry.parked = parked;
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The lowest-latency lane that can carry control frames.
    ///
    /// Ordering by measured latency is what keeps acknowledgements off a Bluetooth lane whenever
    /// anything faster exists: BlueZ spends 5-15 ms per write through D-Bus, which would otherwise
    /// become the transfer's bottleneck.
    pub fn control_lane(&self) -> Result<LaneId, LaneRegistryError> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.profile.carries_control() && !entry.parked)
            .min_by_key(|(_, entry)| entry.rtt_micros)
            .map(|(lane, _)| *lane)
            .ok_or(LaneRegistryError::NoControlLane)
    }

    pub fn bulk_lanes(&self) -> impl Iterator<Item = (LaneId, &LaneEntry)> {
        self.entries
            .iter()
            .filter(|(_, entry)| entry.profile.carries_bulk() && !entry.parked)
            .map(|(lane, entry)| (*lane, entry))
    }

    /// Bulk lanes with contending ones reduced to the fastest in each radio group.
    ///
    /// Two lanes on one chipset split its airtime instead of adding to it, so scheduling both
    /// costs coordination and buys nothing.
    pub fn schedulable_bulk_lanes(&self) -> Vec<LaneId> {
        let mut best_in_group: BTreeMap<GroupKey, (LaneId, u32)> = BTreeMap::new();
        let mut independent = Vec::new();

        for (lane, entry) in self.bulk_lanes() {
            match GroupKey::contended(entry.profile.radio_group) {
                Some(key) => {
                    let candidate = (lane, entry.bandwidth_kbps);
                    best_in_group
                        .entry(key)
                        .and_modify(|best| {
                            if candidate.1 > best.1 {
                                *best = candidate;
                            }
                        })
                        .or_insert(candidate);
                }
                None => independent.push(lane),
            }
        }

        independent.extend(best_in_group.into_values().map(|(lane, _)| lane));
        independent.sort_unstable();
        independent
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum GroupKey {
    Wifi,
    Bluetooth,
    Usb,
}

impl GroupKey {
    const fn contended(group: RadioGroup) -> Option<Self> {
        match group {
            RadioGroup::WifiChipset => Some(Self::Wifi),
            RadioGroup::BluetoothChipset => Some(Self::Bluetooth),
            RadioGroup::Usb => Some(Self::Usb),
            RadioGroup::Loopback => None,
        }
    }
}
