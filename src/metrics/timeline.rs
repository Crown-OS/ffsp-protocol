use std::collections::VecDeque;

use crate::metrics::ThroughputSample;
use crate::wire::LaneId;

/// Throughput over time, per lane, bounded.
///
/// A benchmark wants the shape of a transfer, not only its average: a lane that collapses halfway
/// and a lane that was always slow report the same mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    capacity_per_lane: usize,
    lanes: Vec<(LaneId, VecDeque<ThroughputSample>)>,
}

impl Timeline {
    /// At a sample every 100 ms this is a little under an hour per lane.
    pub const DEFAULT_CAPACITY: usize = 32_768;

    pub const fn new(capacity_per_lane: usize) -> Self {
        Self {
            capacity_per_lane,
            lanes: Vec::new(),
        }
    }

    pub fn push(&mut self, sample: ThroughputSample) {
        let capacity = self.capacity_per_lane;
        let series = match self.lanes.iter_mut().find(|(lane, _)| *lane == sample.lane) {
            Some((_, series)) => series,
            None => {
                self.lanes
                    .push((sample.lane, VecDeque::with_capacity(capacity.min(1024))));
                match self.lanes.last_mut() {
                    Some((_, series)) => series,
                    None => return,
                }
            }
        };
        if series.len() == capacity {
            series.pop_front();
        }
        series.push_back(sample);
    }

    pub fn lanes(&self) -> impl Iterator<Item = LaneId> + '_ {
        self.lanes.iter().map(|(lane, _)| *lane)
    }

    pub fn series(&self, lane: LaneId) -> &[ThroughputSample] {
        self.lanes
            .iter()
            .find(|(id, _)| *id == lane)
            .map(|(_, series)| series.as_slices().0)
            .unwrap_or_default()
    }

    pub fn samples(&self) -> impl Iterator<Item = &ThroughputSample> + '_ {
        self.lanes.iter().flat_map(|(_, series)| series.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.iter().all(|(_, series)| series.is_empty())
    }
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}
