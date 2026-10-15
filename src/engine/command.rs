use std::path::PathBuf;

use crate::control::{EndReason, TransferId};
use crate::device::DeviceId;
use crate::medium::{LaneChannel, Medium};

/// What the platform asks the engine to do.
///
/// Connecting a medium is not in here: discovery and connection belong to the platform, which is
/// the only side that knows what BlueZ or `UsbManager` want. The engine is handed a lane that
/// already works.
#[derive(Debug)]
pub enum EngineCommand {
    /// Hands over a connected medium.
    ///
    /// The first lane to a peer runs the pairing handshake; later ones join the session that
    /// handshake established, at the identifier the initiator assigned.
    AttachLane {
        medium: Medium,
        endpoint: String,
        channel: Box<LaneChannel>,
        /// Whether this end reached out. The initiator drives the handshake and is the only
        /// allocator of lane identifiers thereafter.
        initiate: bool,
        /// Set for a lane joining an established session at an identifier the peer chose.
        adopt: Option<AdoptedLane>,
    },
    /// Offers a file. One file per transfer: the scheduler stripes one payload.
    Offer {
        peer: DeviceId,
        path: PathBuf,
    },
    Accept {
        peer: DeviceId,
        transfer: TransferId,
    },
    Decline {
        peer: DeviceId,
        transfer: TransferId,
        reason: EndReason,
    },
    Cancel {
        peer: DeviceId,
        transfer: TransferId,
        reason: EndReason,
    },
    /// Asks the peer to bring up another medium and join it to the session.
    InviteLane {
        peer: DeviceId,
        medium: Medium,
        endpoint: String,
    },
    /// Brackets a phase in both timelines.
    Mark {
        peer: DeviceId,
        label: String,
    },
    Shutdown,
}

/// A lane joining a session at an identifier the peer already chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdoptedLane {
    pub peer: DeviceId,
    pub lane: crate::wire::LaneId,
}
