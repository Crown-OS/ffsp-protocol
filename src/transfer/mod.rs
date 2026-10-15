//! Splitting a payload into independently verifiable chunks, and moving them over every lane at
//! once.

mod assignment;
mod bitmap;
mod chunk;
mod error;
mod estimator;
mod manifest;
mod probe;
mod resume;
mod scheduler;
mod verify;

pub use assignment::{LaneLoad, earliest_completion};
pub use bitmap::ChunkBitmap;
pub use chunk::{CHUNK_LEN, ChunkLayout};
pub use error::TransferError;
pub use estimator::LaneEstimator;
pub use manifest::{FileEntry, Manifest};
pub use probe::{MarginalContributionProbe, ProbeAction};
pub use resume::ResumeState;
pub use scheduler::{Assignment, ChunkScheduler, LaneStats, Progress};
pub use verify::{ChunkVerifier, chaining_values_of, chunk_chaining_value, fold_root};

#[cfg(test)]
mod tests;
