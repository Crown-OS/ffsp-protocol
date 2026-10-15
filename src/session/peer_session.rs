use std::collections::BTreeMap;

use crate::crypto::{CipherSuite, Direction, LaneCipher, SessionSecret};
use crate::device::{Capabilities, DeviceId};
use crate::medium::{LaneProfile, LaneRegistry};
use crate::session::{SessionError, SessionSignal, SessionState};
use crate::wire::{FrameHeader, FrameKind, LaneId};

/// The two ciphers a lane needs: one per direction, so neither peer waits on the other.
///
/// These leave the session when a lane starts carrying traffic, because a cipher counts nonces in
/// the order it seals and the peer opens in the order frames arrive. Two tasks sealing on one
/// lane would desynchronise that counter and, before it failed, seal two frames under a single
/// nonce. Handing each direction to exactly one task makes that impossible to express rather
/// than merely discouraged.
#[derive(Debug)]
pub struct LaneKeys {
    pub outbound: LaneCipher,
    pub inbound: LaneCipher,
}

/// One linked peer, its lanes, and the keys protecting them.
#[derive(Debug)]
pub struct Session {
    peer: DeviceId,
    secret: SessionSecret,
    direction: Direction,
    suite: CipherSuite,
    state: SessionState,
    lanes: LaneRegistry,
    ciphers: BTreeMap<LaneId, LaneKeys>,
}

impl Session {
    pub const fn new(
        peer: DeviceId,
        secret: SessionSecret,
        direction: Direction,
        local: Capabilities,
        remote: Capabilities,
    ) -> Self {
        Self {
            peer,
            secret,
            direction,
            suite: CipherSuite::negotiate(local, remote),
            state: SessionState::Linking,
            lanes: LaneRegistry::new(),
            ciphers: BTreeMap::new(),
        }
    }

    pub const fn peer(&self) -> DeviceId {
        self.peer
    }

    pub const fn state(&self) -> SessionState {
        self.state
    }

    pub const fn cipher_suite(&self) -> CipherSuite {
        self.suite
    }

    pub const fn lanes(&self) -> &LaneRegistry {
        &self.lanes
    }

    pub fn signal(&mut self, signal: SessionSignal) -> Result<SessionState, SessionError> {
        let next = self
            .state
            .apply(signal)
            .ok_or(SessionError::IllegalTransition {
                state: self.state,
                signal,
            })?;
        self.state = next;
        Ok(next)
    }

    /// Admits a lane at an identifier this end chose, and derives its keys.
    ///
    /// Only the initiator should call this; the responder calls [`Self::adopt_lane`] with the
    /// identifier the initiator sent in its
    /// [`LaneInvite`](crate::control::LaneInvite).
    pub fn register_lane(&mut self, profile: LaneProfile) -> Result<LaneId, SessionError> {
        let lane = self.lanes.allocate(profile)?;
        self.derive_lane_keys(lane);
        Ok(lane)
    }

    /// Admits a lane at the identifier the peer chose.
    pub fn adopt_lane(&mut self, lane: LaneId, profile: LaneProfile) -> Result<(), SessionError> {
        self.lanes.adopt(lane, profile)?;
        self.derive_lane_keys(lane);
        Ok(())
    }

    fn derive_lane_keys(&mut self, lane: LaneId) {
        self.ciphers.insert(
            lane,
            LaneKeys {
                outbound: LaneCipher::new(
                    self.suite,
                    &self.secret.lane_key(self.direction, lane),
                    lane,
                ),
                inbound: LaneCipher::new(
                    self.suite,
                    &self.secret.lane_key(self.direction.peer(), lane),
                    lane,
                ),
            },
        );
    }

    /// Hands a lane's ciphers to whoever will drive it, leaving the lane registered.
    ///
    /// After this the session can no longer seal or open on that lane; the tasks that hold the
    /// ciphers do, which is the point.
    pub fn take_lane_keys(&mut self, lane: LaneId) -> Option<LaneKeys> {
        self.ciphers.remove(&lane)
    }

    /// Drops a lane and its keys. The identifier is never handed out again.
    pub fn release_lane(&mut self, lane: LaneId) {
        self.lanes.remove(lane);
        self.ciphers.remove(&lane);
    }

    pub fn seal(
        &mut self,
        lane: LaneId,
        kind: FrameKind,
        body: &mut impl aes_gcm::aead::Buffer,
    ) -> Result<[u8; FrameHeader::LEN], SessionError> {
        if !self.state.can_move_data() {
            return Err(SessionError::NotTransferable(self.state));
        }
        let ciphers = self
            .ciphers
            .get_mut(&lane)
            .ok_or(SessionError::UnknownLane { lane })?;
        Ok(ciphers.outbound.seal(kind, body)?)
    }

    pub fn open<'a>(
        &mut self,
        header: &FrameHeader,
        sealed: &'a mut [u8],
    ) -> Result<&'a [u8], SessionError> {
        let lane = header.lane();
        let ciphers = self
            .ciphers
            .get_mut(&lane)
            .ok_or(SessionError::UnknownLane { lane })?;
        Ok(ciphers.inbound.open(header, sealed)?)
    }

    pub fn control_lane(&self) -> Result<LaneId, SessionError> {
        Ok(self.lanes.control_lane()?)
    }

    /// Bulk lanes worth scheduling, with contending radios already reduced to one each.
    pub fn schedulable_bulk_lanes(&self) -> Vec<LaneId> {
        self.lanes.schedulable_bulk_lanes()
    }
}
