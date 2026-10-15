use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::device::{Capabilities, DeviceClass, DeviceId};
use crate::medium::{LaneProfile, Medium};

/// One observation of a peer over one medium.
#[derive(Debug, Clone)]
pub struct Sighting<'a> {
    pub device: DeviceId,
    pub class: DeviceClass,
    pub capabilities: Capabilities,
    pub medium: Medium,
    /// Where to reach the peer over this medium: a Bluetooth address, a `host:port`, a USB path.
    pub endpoint: Cow<'a, str>,
    pub observed_at_millis: u64,
    pub signal_strength: Option<i8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediumSighting {
    pub endpoint: String,
    pub observed_at_millis: u64,
    pub signal_strength: Option<i8>,
}

/// A linked device currently within reach, with every medium that can see it.
///
/// One peer, not one per radio: a phone seen over Bluetooth, mDNS and USB at once is a single
/// entry here. The identity that merges them is the [`DeviceId`] the beacon resolved to, which is
/// why the BLE and mDNS planes deliberately carry the same payload.
#[derive(Debug, Clone)]
pub struct NearbyPeer {
    device: DeviceId,
    class: DeviceClass,
    capabilities: Capabilities,
    sightings: BTreeMap<Medium, MediumSighting>,
}

impl NearbyPeer {
    pub const fn device(&self) -> DeviceId {
        self.device
    }

    pub const fn class(&self) -> DeviceClass {
        self.class
    }

    pub const fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    pub fn mediums(&self) -> impl Iterator<Item = Medium> {
        self.sightings.keys().copied()
    }

    pub fn sighting(&self, medium: Medium) -> Option<&MediumSighting> {
        self.sightings.get(&medium)
    }

    pub fn last_seen_millis(&self) -> u64 {
        self.sightings
            .values()
            .map(|s| s.observed_at_millis)
            .max()
            .unwrap_or_default()
    }

    /// The medium worth opening a lane on first.
    pub fn fastest_medium(&self) -> Option<Medium> {
        self.mediums()
            .max_by_key(|medium| LaneProfile::of(*medium).bandwidth_hint_kbps)
    }
}

/// What changed when a sighting was recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceChange {
    /// The peer was not in reach, and now is.
    Appeared(DeviceId),
    /// A known peer became reachable over an additional medium, which is the cue to consider
    /// adding a lane to a session already in progress.
    MediumAdded { device: DeviceId, medium: Medium },
    /// A sighting this tracker already had, refreshed.
    Refreshed(DeviceId),
}

/// Which linked devices are in reach, over which mediums.
#[derive(Debug)]
pub struct PresenceTracker {
    ttl_millis: u64,
    peers: BTreeMap<DeviceId, NearbyPeer>,
}

impl PresenceTracker {
    /// Bluetooth scan results arrive every few seconds when foregrounded and far less often when
    /// throttled, so a peer is kept for well over one advertising interval.
    pub const DEFAULT_TTL_MILLIS: u64 = 30_000;

    pub const fn new(ttl_millis: u64) -> Self {
        Self {
            ttl_millis,
            peers: BTreeMap::new(),
        }
    }

    pub fn observe(&mut self, sighting: Sighting<'_>) -> PresenceChange {
        let Sighting {
            device,
            class,
            capabilities,
            medium,
            endpoint,
            observed_at_millis,
            signal_strength,
        } = sighting;
        let entry = MediumSighting {
            endpoint: endpoint.into_owned(),
            observed_at_millis,
            signal_strength,
        };

        match self.peers.get_mut(&device) {
            Some(peer) => {
                peer.class = class;
                peer.capabilities = capabilities;
                let is_new_medium = peer.sightings.insert(medium, entry).is_none();
                if is_new_medium {
                    PresenceChange::MediumAdded { device, medium }
                } else {
                    PresenceChange::Refreshed(device)
                }
            }
            None => {
                self.peers.insert(
                    device,
                    NearbyPeer {
                        device,
                        class,
                        capabilities,
                        sightings: BTreeMap::from([(medium, entry)]),
                    },
                );
                PresenceChange::Appeared(device)
            }
        }
    }

    /// Drops stale sightings, returning the peers that went out of reach entirely.
    pub fn expire(&mut self, now_millis: u64) -> Vec<DeviceId> {
        let cutoff = now_millis.saturating_sub(self.ttl_millis);
        for peer in self.peers.values_mut() {
            peer.sightings
                .retain(|_, sighting| sighting.observed_at_millis > cutoff);
        }
        let lost: Vec<DeviceId> = self
            .peers
            .values()
            .filter(|peer| peer.sightings.is_empty())
            .map(NearbyPeer::device)
            .collect();
        self.peers.retain(|_, peer| !peer.sightings.is_empty());
        lost
    }

    /// Forgets one medium's sighting, for when a driver reports the endpoint gone rather than
    /// merely letting it time out.
    pub fn withdraw(&mut self, device: &DeviceId, medium: Medium) -> Option<PresenceChange> {
        let peer = self.peers.get_mut(device)?;
        peer.sightings.remove(&medium)?;
        if peer.sightings.is_empty() {
            self.peers.remove(device);
        }
        Some(PresenceChange::Refreshed(*device))
    }

    pub fn get(&self, device: &DeviceId) -> Option<&NearbyPeer> {
        self.peers.get(device)
    }

    pub fn nearby(&self) -> impl Iterator<Item = &NearbyPeer> {
        self.peers.values()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

impl Default for PresenceTracker {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TTL_MILLIS)
    }
}
