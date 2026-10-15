//! What actually happened, measured where it happened.
//!
//! [`transfer`](crate::transfer) knows what it predicted and what it scheduled; this records what
//! arrived, on which medium, when. The two views are kept apart rather than reconciled, because
//! the difference between them is the acknowledgement path and that is worth reading.

mod lane_meter;
mod lane_summary;
mod sample;
mod setup_timing;
mod timeline;
mod transfer_report;

pub use lane_meter::{LaneMeter, ParkEvent, rate_kbps};
pub use lane_summary::LaneSummary;
pub use sample::{RttSummary, ThroughputSample};
pub use setup_timing::{Phase, SetupTiming};
pub use timeline::Timeline;
pub use transfer_report::{
    CalibrationReport, RunConditions, StandaloneRun, TransferReport, TransferRole,
};

#[cfg(test)]
mod tests;
