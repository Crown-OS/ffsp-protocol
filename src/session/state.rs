/// Where a session is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Exchanging keys and waiting for a person to confirm the verification code.
    Linking,
    /// Linked, at least one lane open, nothing in flight.
    Ready,
    Transferring,
    /// Every lane has gone, but the transfer's progress is intact and will resume.
    Suspended,
    Closed,
}

/// Something that happened to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSignal {
    Linked,
    LaneAvailable,
    AllLanesLost,
    TransferStarted,
    TransferFinished,
    Closed,
}

impl SessionState {
    /// Applies a signal, or reports that it makes no sense here.
    ///
    /// Losing every lane suspends rather than fails, which is what lets a transfer survive a radio
    /// outage or a phone being carried out of range and back: the chunk bitmap is untouched, so
    /// reconnecting costs the chunks that were in flight and nothing more.
    pub const fn apply(self, signal: SessionSignal) -> Option<Self> {
        match (self, signal) {
            (_, SessionSignal::Closed) => Some(Self::Closed),
            (Self::Closed, _) => None,

            (Self::Linking, SessionSignal::Linked) => Some(Self::Ready),
            (Self::Linking, _) => None,

            (Self::Ready | Self::Suspended, SessionSignal::LaneAvailable) => Some(Self::Ready),
            (Self::Transferring, SessionSignal::LaneAvailable) => Some(Self::Transferring),

            (Self::Ready | Self::Suspended, SessionSignal::TransferStarted) => {
                Some(Self::Transferring)
            }
            (Self::Transferring, SessionSignal::TransferFinished) => Some(Self::Ready),

            (Self::Ready, SessionSignal::AllLanesLost) => Some(Self::Ready),
            (Self::Transferring, SessionSignal::AllLanesLost) => Some(Self::Suspended),
            (Self::Suspended, SessionSignal::AllLanesLost) => Some(Self::Suspended),

            _ => None,
        }
    }

    pub const fn can_move_data(self) -> bool {
        matches!(self, Self::Ready | Self::Transferring)
    }

    pub const fn is_final(self) -> bool {
        matches!(self, Self::Closed)
    }
}
