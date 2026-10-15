use crate::control::TransferId;
use crate::crypto::CipherSuite;
use crate::device::DeviceId;
use crate::link::VerificationCode;
use crate::medium::Medium;
use crate::session::SessionState;
use crate::transfer::{Manifest, Progress};
use crate::wire::LaneId;

/// One lane's contribution to a transfer.
///
/// Reported per lane rather than only in aggregate because this is the only way to tell striping
/// from failover: a lane sitting at zero bytes means the scheduler never used it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneProgress {
    pub lane: LaneId,
    pub medium: Medium,
    pub delivered_bytes: u64,
    pub bandwidth_kbps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferOutcome {
    Completed,
    Declined,
    Cancelled,
    /// Every lane went away and the progress was persisted for a later resume.
    Suspended,
}

/// Something worth telling the platform about.
///
/// This is the whole outward surface of the engine. Events are pulled rather than delivered
/// through a callback, so no worker thread of this crate ever has to enter the platform's runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    PeerAppeared {
        device: DeviceId,
        medium: Medium,
    },
    PeerLost {
        device: DeviceId,
    },

    /// A person has to compare this code on both devices before the link is accepted.
    LinkRequested {
        device: DeviceId,
        code: VerificationCode,
    },
    LinkSettled {
        device: DeviceId,
        accepted: bool,
    },

    SessionChanged {
        device: DeviceId,
        state: SessionState,
    },

    LaneOpened {
        device: DeviceId,
        lane: LaneId,
        medium: Medium,
    },
    LaneClosed {
        device: DeviceId,
        lane: LaneId,
    },
    /// The probe measured the transfer going faster without this lane.
    LaneParked {
        device: DeviceId,
        lane: LaneId,
    },
    /// The engine wants a lane and the platform should open one, which keeps every call across
    /// the boundary flowing in a single direction.
    LaneNeeded {
        device: DeviceId,
        medium: Medium,
    },

    /// `transfer` is what [`EngineCommand::Accept`](super::EngineCommand::Accept) must name.
    TransferOffered {
        device: DeviceId,
        transfer: TransferId,
        manifest: Manifest,
    },
    TransferProgress {
        device: DeviceId,
        progress: Progress,
        lanes: Vec<LaneProgress>,
    },
    TransferFinished {
        device: DeviceId,
        outcome: TransferOutcome,
    },

    /// Carries the platform's own action vocabulary, which this crate never interprets.
    ControlFrame {
        device: DeviceId,
        payload: Vec<u8>,
    },

    /// Everything one end measured, once a transfer ended.
    TransferReport {
        device: DeviceId,
        report: Box<crate::metrics::TransferReport>,
    },
    /// The peer asked for another medium, and named the lane it will answer to.
    LaneInvited {
        device: DeviceId,
        lane: LaneId,
        medium: Medium,
        endpoint: String,
    },
    Diagnostic(Diagnostic),
}

/// Facts worth logging that are not errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diagnostic {
    /// Emitted at session start so a build that silently lost its AES intrinsics is visible in a
    /// log rather than only as an unexplained throughput drop.
    CipherSuiteChosen {
        device: DeviceId,
        suite: CipherSuite,
        aes_hardware: bool,
    },
    /// The destination cannot take writes at an offset, so chunks for it must be ordered.
    PositionalWritesUnavailable { entry: String },
    /// A beacon failed to resolve and reported an epoch outside the window.
    ClockSkewSuspected { epoch_low: u8 },
    /// A medium was handed over but never became usable.
    LaneFailed {
        medium: Medium,
        endpoint: String,
        reason: String,
    },
    /// Something the engine could not carry out, named rather than swallowed.
    EngineFailed { reason: String },
}
