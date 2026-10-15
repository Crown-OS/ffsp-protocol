use std::collections::BTreeMap;
use std::fs::File;
use std::sync::Arc;

use blake3::Hash;
use blake3::hazmat::ChainingValue;
use tokio::sync::mpsc;

use crate::control::TransferId;
use crate::engine::inbound::SharedSink;
use crate::engine::outbound::Outbound;
use crate::medium::LaneProfile;
use crate::metrics::{LaneMeter, SetupTiming, Timeline};
use crate::session::Session;
use crate::transfer::{ChunkBitmap, ChunkLayout, ChunkScheduler};
use crate::wire::LaneId;

/// One attached lane, from the engine's side of it.
#[derive(Debug)]
pub(crate) struct LaneHandle {
    pub(crate) profile: LaneProfile,
    /// Chunks. Deep, because the scheduler decides how much may be outstanding.
    pub(crate) bulk: mpsc::UnboundedSender<Outbound>,
    /// Acknowledgements, keepalives and control, which overtake queued chunks.
    pub(crate) priority: mpsc::UnboundedSender<Outbound>,
    pub(crate) meter: LaneMeter,
    /// The ping this lane is waiting on, and when it left.
    pub(crate) pending_ping: Option<(u64, u64)>,
    pub(crate) next_nonce: u64,
    pub(crate) last_ping_micros: u64,
}

impl LaneHandle {
    pub(crate) fn send_bulk(&self, item: Outbound) -> bool {
        self.bulk.send(item).is_ok()
    }

    pub(crate) fn send_priority(&self, item: Outbound) -> bool {
        self.priority.send(item).is_ok()
    }
}

/// A payload this end is sending.
#[derive(Debug)]
pub(crate) struct OutboundTransfer {
    pub(crate) id: TransferId,
    pub(crate) scheduler: ChunkScheduler,
    pub(crate) layout: ChunkLayout,
    pub(crate) source: Arc<File>,
    pub(crate) name: String,
    pub(crate) chaining_values: Vec<ChainingValue>,
    pub(crate) started_micros: u64,
    /// The peer has agreed, but may not yet hold the verification tree.
    pub(crate) accepted: bool,
    /// The peer can verify a chunk the moment it lands.
    pub(crate) ready: bool,
    /// Reads in flight, so the pump knows the pool is busier than it looks.
    pub(crate) loading: usize,
    pub(crate) abandoned: u32,
}

/// A payload this end is receiving.
#[derive(Debug)]
pub(crate) struct InboundTransfer {
    pub(crate) id: TransferId,
    pub(crate) layout: ChunkLayout,
    pub(crate) bitmap: ChunkBitmap,
    pub(crate) name: String,
    pub(crate) root: Hash,
    pub(crate) chaining_values: Vec<ChainingValue>,
    /// The first index of each page that has landed, so completeness is counted rather than
    /// inferred from the values themselves.
    pub(crate) pages: std::collections::BTreeSet<u32>,
    pub(crate) destination: Arc<File>,
    pub(crate) started_micros: u64,
    /// The verification tree is complete and chunks are being written.
    pub(crate) ready: bool,
}

impl InboundTransfer {
    /// Whether verification can begin.
    ///
    /// A payload of one chunk has no tree: its root is the chunk's own hash, and the verifier
    /// wants an empty vector.
    pub(crate) fn tree_is_complete(&self) -> bool {
        if self.layout.is_single_subtree() {
            return true;
        }
        let expected =
            (self.layout.count() as usize).div_ceil(crate::control::CHUNK_MAP_PAGE_LEN as usize);
        self.pages.len() == expected
    }

    pub(crate) fn absorb_page(&mut self, first: u32, values: &[ChainingValue]) {
        self.pages.insert(first);
        let needed = self.layout.count() as usize;
        if self.chaining_values.len() != needed {
            self.chaining_values.resize(needed, [0u8; 32]);
        }
        for (offset, value) in values.iter().enumerate() {
            let Some(slot) = self.chaining_values.get_mut(first as usize + offset) else {
                continue;
            };
            *slot = *value;
        }
    }
}

/// Everything the engine holds about one peer.
#[derive(Debug)]
pub(crate) struct PeerState {
    pub(crate) session: Session,
    pub(crate) lanes: BTreeMap<LaneId, LaneHandle>,
    /// Shared with every reader on this peer, so they know where verified chunks go.
    pub(crate) sink: SharedSink,
    pub(crate) outbound: Option<OutboundTransfer>,
    pub(crate) inbound: Option<InboundTransfer>,
    pub(crate) next_transfer: TransferId,
    pub(crate) setup: SetupTiming,
    pub(crate) timeline: Timeline,
    /// Whether this end allocates lane identifiers.
    pub(crate) allocates_lanes: bool,
}

impl PeerState {
    /// The lane control frames should take: the lowest measured round trip that can carry them.
    pub(crate) fn control_lane(&self) -> Option<LaneId> {
        self.session
            .control_lane()
            .ok()
            .filter(|lane| self.lanes.contains_key(lane))
            .or_else(|| self.lanes.keys().next().copied())
    }
}
