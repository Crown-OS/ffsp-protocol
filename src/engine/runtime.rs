use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use blake3::hazmat::ChainingValue;
use tokio::sync::mpsc;

use crate::control::{
    ChunkAck, ChunkMapPage, ControlMessage, Delivery, EndReason, KeepAlive, LaneInvite, LaneReport,
    TransferId,
};
use crate::crypto::has_aes_hardware;
use crate::device::DeviceId;
use crate::engine::command::{AdoptedLane, EngineCommand};
use crate::engine::config::EngineConfig;
use crate::engine::error::EngineError;
use crate::engine::event::{Diagnostic, EngineEvent, LaneProgress, TransferOutcome};
use crate::engine::handshake::{self, FIRST_LANE};
use crate::engine::inbound::{Inbound, InboundKind, InboundSink, LaneReader, SharedSink};
use crate::engine::outbound::{LaneNotice, LaneWriter, Outbound};
use crate::engine::peer::{InboundTransfer, LaneHandle, OutboundTransfer, PeerState};
use crate::io::{ChunkBuffer, ChunkBufferPool, PositionalRead, reserve, supports_positional_write};
use crate::link::{HandshakeOutcome, LinkRecord};
use crate::medium::{LaneChannel, LaneProfile, Medium};
use crate::metrics::{
    LaneMeter, LaneSummary, RunConditions, SetupTiming, Timeline, TransferReport, TransferRole,
};
use crate::session::{Session, SessionSignal};
use crate::transfer::{
    CHUNK_LEN, ChunkBitmap, ChunkLayout, ChunkScheduler, ChunkVerifier, FileEntry, Manifest,
};
use crate::wire::{ChunkIndex, FrameKind, LaneId, chunk_header_bytes};

/// A lane that finished whatever had to happen before it could carry traffic.
#[derive(Debug)]
pub(crate) enum LaneReady {
    Handshaken {
        channel: Box<LaneChannel>,
        outcome: Box<HandshakeOutcome>,
        medium: Medium,
        endpoint: String,
        initiate: bool,
    },
    Failed {
        medium: Medium,
        endpoint: String,
        reason: String,
    },
}

/// A chunk read off disk and ready to seal.
#[derive(Debug)]
pub(crate) struct Loaded {
    peer: DeviceId,
    lane: LaneId,
    index: ChunkIndex,
    buffer: Option<ChunkBuffer>,
    error: Option<String>,
}

pub(crate) struct Runtime {
    config: EngineConfig,
    pool: Arc<ChunkBufferPool>,
    peers: BTreeMap<DeviceId, PeerState>,
    events: mpsc::UnboundedSender<EngineEvent>,
    inbound: (
        mpsc::UnboundedSender<Inbound>,
        mpsc::UnboundedReceiver<Inbound>,
    ),
    notices: (
        mpsc::UnboundedSender<LaneNotice>,
        mpsc::UnboundedReceiver<LaneNotice>,
    ),
    ready: (
        mpsc::UnboundedSender<LaneReady>,
        mpsc::UnboundedReceiver<LaneReady>,
    ),
    loaded: (
        mpsc::UnboundedSender<Loaded>,
        mpsc::UnboundedReceiver<Loaded>,
    ),
    /// Which peer and lane each outstanding read belongs to.
    last_sample_micros: u64,
}

impl Runtime {
    pub(crate) fn new(config: EngineConfig, events: mpsc::UnboundedSender<EngineEvent>) -> Self {
        let pool = Arc::new(ChunkBufferPool::new(
            config.tuning.buffer_pool_size,
            config.tuning.buffer_capacity(),
        ));
        Self {
            config,
            pool,
            peers: BTreeMap::new(),
            events,
            inbound: mpsc::unbounded_channel(),
            notices: mpsc::unbounded_channel(),
            ready: mpsc::unbounded_channel(),
            loaded: mpsc::unbounded_channel(),
            last_sample_micros: 0,
        }
    }

    pub(crate) async fn run(mut self, mut commands: mpsc::UnboundedReceiver<EngineCommand>) {
        let mut ticker = tokio::time::interval(Duration::from_millis(
            self.config.sample_interval_millis.max(10),
        ));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            let stop = tokio::select! {
                command = commands.recv() => match command {
                    Some(EngineCommand::Shutdown) | None => true,
                    Some(command) => { self.on_command(command); false }
                },
                Some(event) = self.inbound.1.recv() => { self.on_inbound(event); false }
                Some(notice) = self.notices.1.recv() => { self.on_notice(notice); false }
                Some(ready) = self.ready.1.recv() => { self.on_lane_ready(ready); false }
                Some(loaded) = self.loaded.1.recv() => { self.on_loaded(loaded); false }
                _ = ticker.tick() => { self.on_tick(); false }
            };
            if stop {
                break;
            }
            self.pump();
        }
        self.close_all();
    }

    // --- MARK: commands ---

    fn on_command(&mut self, command: EngineCommand) {
        let outcome = match command {
            EngineCommand::AttachLane {
                medium,
                endpoint,
                channel,
                initiate,
                adopt,
            } => self.attach_lane(medium, endpoint, channel, initiate, adopt),
            EngineCommand::Offer { peer, path } => self.offer(peer, &path),
            EngineCommand::Accept { peer, transfer } => self.accept(peer, transfer),
            EngineCommand::Decline {
                peer,
                transfer,
                reason,
            } => self.end_transfer(peer, transfer, reason, false),
            EngineCommand::Cancel {
                peer,
                transfer,
                reason,
            } => self.end_transfer(peer, transfer, reason, true),
            EngineCommand::InviteLane {
                peer,
                medium,
                endpoint,
            } => self.invite_lane(peer, medium, endpoint),
            EngineCommand::Mark { peer, label } => self.mark(peer, label),
            EngineCommand::Shutdown => Ok(()),
        };
        if let Err(error) = outcome {
            self.emit(EngineEvent::Diagnostic(Diagnostic::EngineFailed {
                reason: error.to_string(),
            }));
        }
    }

    fn attach_lane(
        &mut self,
        medium: Medium,
        endpoint: String,
        channel: Box<LaneChannel>,
        initiate: bool,
        adopt: Option<AdoptedLane>,
    ) -> Result<(), EngineError> {
        if let Some(adopted) = adopt {
            let profile = LaneProfile::of(medium);
            let peer = self
                .peers
                .get_mut(&adopted.peer)
                .ok_or(EngineError::UnknownPeer(adopted.peer))?;
            // The inviting end registered this identifier when it chose it; the other end is
            // meeting it for the first time.
            if peer.session.lanes().get(adopted.lane).is_none() {
                peer.session.adopt_lane(adopted.lane, profile)?;
            }
            return self.start_lane(adopted.peer, adopted.lane, medium, endpoint, *channel);
        }

        let identity = self.config.identity.clone();
        let ready = self.ready.0.clone();
        let mut channel = channel;
        tokio::spawn(async move {
            let message = match handshake::perform(&mut channel, &identity, initiate, None).await {
                Ok(outcome) => LaneReady::Handshaken {
                    channel,
                    outcome: Box::new(outcome),
                    medium,
                    endpoint,
                    initiate,
                },
                Err(error) => LaneReady::Failed {
                    medium,
                    endpoint,
                    reason: error.to_string(),
                },
            };
            let _ = ready.send(message);
        });
        Ok(())
    }

    fn on_lane_ready(&mut self, ready: LaneReady) {
        let result = match ready {
            LaneReady::Failed {
                medium,
                endpoint,
                reason,
            } => {
                self.emit(EngineEvent::Diagnostic(Diagnostic::LaneFailed {
                    medium,
                    endpoint,
                    reason,
                }));
                Ok(())
            }
            LaneReady::Handshaken {
                channel,
                outcome,
                medium,
                endpoint,
                initiate,
            } => self.settle_session(*channel, *outcome, medium, endpoint, initiate),
        };
        if let Err(error) = result {
            self.emit(EngineEvent::Diagnostic(Diagnostic::EngineFailed {
                reason: error.to_string(),
            }));
        }
    }

    fn settle_session(
        &mut self,
        channel: LaneChannel,
        outcome: HandshakeOutcome,
        medium: Medium,
        endpoint: String,
        initiate: bool,
    ) -> Result<(), EngineError> {
        let device = outcome.peer;
        let remembered = self.config.links.load(&device)?;
        let capabilities = remembered
            .as_ref()
            .map(|record| record.capabilities)
            .unwrap_or(self.config.capabilities);

        // A fresh pairing yields a link secret; keep the one already stored so the discovery
        // beacons a peer is already listening for do not change under it.
        if let Some(secret) = outcome.link_secret.clone()
            && remembered.is_none()
        {
            let name = self.config.device_name.clone();
            let record = LinkRecord::new(
                outcome.peer_public_key,
                name,
                self.config.device_class,
                capabilities,
                secret,
            );
            self.config.links.store(&record)?;
        }

        let mut session = Session::new(
            device,
            outcome.session_secret,
            outcome.direction,
            self.config.capabilities,
            capabilities,
        );
        session.signal(SessionSignal::Linked)?;
        if initiate {
            let lane = session.register_lane(LaneProfile::of(medium))?;
            debug_assert_eq!(lane, FIRST_LANE);
        } else {
            session.adopt_lane(FIRST_LANE, LaneProfile::of(medium))?;
        }

        let suite = session.cipher_suite();
        let mut setup = SetupTiming::new();
        setup.begin("session", self.now_micros());
        setup.end("session", self.now_micros());

        self.peers.insert(
            device,
            PeerState {
                session,
                lanes: BTreeMap::new(),
                sink: Arc::new(Mutex::new(None)),
                outbound: None,
                inbound: None,
                next_transfer: TransferId::new(1),
                setup,
                timeline: Timeline::default(),
                allocates_lanes: initiate,
            },
        );

        self.emit(EngineEvent::Diagnostic(Diagnostic::CipherSuiteChosen {
            device,
            suite,
            aes_hardware: has_aes_hardware(),
        }));
        self.emit(EngineEvent::LinkSettled {
            device,
            accepted: true,
        });
        self.start_lane(device, FIRST_LANE, medium, endpoint, channel)
    }

    /// Splits a lane between its two tasks and starts them.
    fn start_lane(
        &mut self,
        device: DeviceId,
        lane: LaneId,
        medium: Medium,
        endpoint: String,
        channel: LaneChannel,
    ) -> Result<(), EngineError> {
        let now = self.now_micros();
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let keys = peer
            .session
            .take_lane_keys(lane)
            .ok_or(EngineError::UnknownLane { lane })?;
        let profile = *channel.profile();
        let mut channel = channel;
        channel.relabel(lane);
        let (write, read) = channel.into_halves();

        let (bulk_tx, bulk_rx) = mpsc::unbounded_channel();
        let (priority_tx, priority_rx) = mpsc::unbounded_channel();

        let writer = LaneWriter::new(
            device,
            write,
            keys.outbound,
            Arc::clone(&self.pool),
            Arc::clone(&self.config.clock),
            self.notices.0.clone(),
        );
        tokio::spawn(writer.run(bulk_rx, priority_rx));

        let reader = LaneReader::new(
            device,
            read,
            keys.inbound,
            Arc::clone(&self.config.clock),
            Arc::clone(&peer.sink),
            self.inbound.0.clone(),
        );
        tokio::spawn(reader.run());

        peer.lanes.insert(
            lane,
            LaneHandle {
                profile,
                bulk: bulk_tx,
                priority: priority_tx,
                meter: LaneMeter::new(lane, medium, endpoint, now),
                pending_ping: None,
                next_nonce: 1,
                last_ping_micros: 0,
            },
        );
        if let Some(transfer) = peer.outbound.as_mut() {
            transfer.scheduler.add_lane(lane, &profile);
        }
        let _ = peer.session.signal(SessionSignal::LaneAvailable);
        self.emit(EngineEvent::LaneOpened {
            device,
            lane,
            medium,
        });
        Ok(())
    }

    fn invite_lane(
        &mut self,
        device: DeviceId,
        medium: Medium,
        endpoint: String,
    ) -> Result<(), EngineError> {
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        if !peer.allocates_lanes {
            return Err(EngineError::UnknownLane {
                lane: LaneId::HANDSHAKE,
            });
        }
        let lane = peer.session.register_lane(LaneProfile::of(medium))?;
        let message = ControlMessage::LaneInvite(LaneInvite {
            lane,
            medium,
            endpoint: endpoint.clone(),
        });
        Self::send_control(peer, &message)?;
        // Both ends are told the same thing: this identifier is reserved for this medium, and
        // the platform should attach its half when the medium is up.
        self.emit(EngineEvent::LaneInvited {
            device,
            lane,
            medium,
            endpoint,
        });
        Ok(())
    }

    fn mark(&mut self, device: DeviceId, label: String) -> Result<(), EngineError> {
        let now = self.now_micros();
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        peer.setup.begin(label.clone(), now);
        Self::send_control(peer, &ControlMessage::BenchmarkMark { label })
    }

    // --- MARK: offering and accepting ---

    fn offer(&mut self, device: DeviceId, path: &Path) -> Result<(), EngineError> {
        let now = self.now_micros();
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        let layout = ChunkLayout::new(len)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "payload".to_owned());

        let source = Arc::new(file);
        let (chaining_values, root) = build_tree(&source, &layout)?;
        let entry = FileEntry::new(name.clone(), len, root)?;
        let manifest = Manifest::new(vec![entry]);

        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let transfer = peer.next_transfer;
        peer.next_transfer = transfer.next();

        let mut scheduler = ChunkScheduler::new(layout);
        scheduler.set_max_inflight_per_lane(self.config.tuning.max_inflight_per_lane);
        for (lane, handle) in &peer.lanes {
            if handle.profile.carries_bulk() {
                scheduler.add_lane(*lane, &handle.profile);
            }
        }

        peer.outbound = Some(OutboundTransfer {
            id: transfer,
            scheduler,
            layout,
            source,
            name,
            chaining_values,
            started_micros: now,
            accepted: false,
            ready: false,
            loading: 0,
            abandoned: 0,
        });
        peer.setup.begin("offer", now);
        Self::send_control(peer, &ControlMessage::TransferOffer { transfer, manifest })?;
        let _ = peer.session.signal(SessionSignal::TransferStarted);
        Ok(())
    }

    fn accept(&mut self, device: DeviceId, transfer: TransferId) -> Result<(), EngineError> {
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let inbound = peer
            .inbound
            .as_ref()
            .filter(|inbound| inbound.id == transfer)
            .ok_or(EngineError::NoTransfer(device))?;
        let _ = inbound;
        Self::send_control(
            peer,
            &ControlMessage::TransferAccept {
                transfer,
                resume: None,
            },
        )
    }

    fn end_transfer(
        &mut self,
        device: DeviceId,
        transfer: TransferId,
        reason: EndReason,
        started: bool,
    ) -> Result<(), EngineError> {
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let message = if started {
            ControlMessage::TransferCancel { transfer, reason }
        } else {
            ControlMessage::TransferDecline { transfer, reason }
        };
        Self::send_control(peer, &message)?;
        peer.inbound = None;
        peer.outbound = None;
        *sink_slot(&peer.sink) = None;
        self.emit(EngineEvent::TransferFinished {
            device,
            outcome: if started {
                TransferOutcome::Cancelled
            } else {
                TransferOutcome::Declined
            },
        });
        Ok(())
    }

    // --- MARK: plumbing ---

    fn send_control(peer: &PeerState, message: &ControlMessage) -> Result<(), EngineError> {
        let lane = peer.control_lane().ok_or(EngineError::UnknownLane {
            lane: LaneId::HANDSHAKE,
        })?;
        let handle = peer
            .lanes
            .get(&lane)
            .ok_or(EngineError::UnknownLane { lane })?;
        handle.send_priority(Outbound::Frame {
            kind: FrameKind::Control,
            body: message.to_bytes()?,
        });
        Ok(())
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }

    fn now_micros(&self) -> u64 {
        self.config.clock.now_micros()
    }

    fn close_all(&mut self) {
        for peer in self.peers.values() {
            for handle in peer.lanes.values() {
                handle.send_priority(Outbound::Close);
            }
        }
        self.peers.clear();
    }
}

/// Reads a whole payload once to build its verification tree and root.
///
/// The tree has to exist before the first chunk moves, because a chunk is only verifiable
/// against it. For a benchmark rig this pass is also the fairest possible warm-up: it puts the
/// payload in the page cache, so the transfer measures the links rather than the disk.
///
/// A payload of a single chunk has no tree at all — its root is the chunk's own hash — which is
/// what [`ChunkVerifier::new`] expects to be handed an empty vector for.
fn build_tree(
    source: &Arc<File>,
    layout: &ChunkLayout,
) -> Result<(Vec<ChainingValue>, blake3::Hash), EngineError> {
    let mut scratch = vec![0u8; CHUNK_LEN as usize];
    let capacity = scratch.len();
    let mut values = Vec::with_capacity(layout.count() as usize);
    let mut whole = blake3::Hasher::new();

    for index in layout.indices() {
        let len = layout
            .len_at(index)
            .ok_or(EngineError::ChunkOutOfRange { index })? as usize;
        let offset = layout
            .byte_offset(index)
            .ok_or(EngineError::ChunkOutOfRange { index })?;
        let slice = scratch
            .get_mut(..len)
            .ok_or(EngineError::ChunkTooLong { len, capacity })?;
        source.read_exact_at(offset, slice)?;
        if layout.is_single_subtree() {
            whole.update(slice);
        } else {
            values.push(crate::transfer::chunk_chaining_value(offset, slice)?);
        }
    }

    let root = if layout.is_single_subtree() {
        whole.finalize()
    } else {
        crate::transfer::fold_root(&values, layout)?
    };
    Ok((values, root))
}

fn sink_slot(sink: &SharedSink) -> std::sync::MutexGuard<'_, Option<Arc<InboundSink>>> {
    match sink.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Runtime {
    // --- MARK: inbound ---

    fn on_inbound(&mut self, event: Inbound) {
        let Inbound { device, lane, kind } = event;
        let result = match kind {
            InboundKind::Chunk {
                transfer,
                index,
                payload_bytes,
                frame_bytes,
                arrived_at_micros,
            } => self.on_chunk(
                device,
                lane,
                transfer,
                index,
                payload_bytes,
                frame_bytes,
                arrived_at_micros,
            ),
            InboundKind::Control(message) => self.on_control(device, lane, *message),
            InboundKind::Platform(payload) => {
                self.emit(EngineEvent::ControlFrame { device, payload });
                Ok(())
            }
            InboundKind::Ack(ack) => self.on_ack(device, &ack),
            InboundKind::Report(report) => self.on_report(device, lane, report),
            InboundKind::KeepAlive {
                keepalive,
                at_micros,
            } => self.on_keepalive(device, lane, keepalive, at_micros),
            InboundKind::Closed { reason } => self.on_lane_closed(device, lane, reason),
        };
        if let Err(error) = result {
            self.emit(EngineEvent::Diagnostic(Diagnostic::EngineFailed {
                reason: error.to_string(),
            }));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_chunk(
        &mut self,
        device: DeviceId,
        lane: LaneId,
        transfer: TransferId,
        index: ChunkIndex,
        payload_bytes: u64,
        frame_bytes: u64,
        arrived_at_micros: u64,
    ) -> Result<(), EngineError> {
        let events = self.events.clone();
        let Some(peer) = self.peers.get_mut(&device) else {
            return Ok(());
        };
        if let Some(handle) = peer.lanes.get_mut(&lane) {
            handle.meter.record_received_frame(frame_bytes);
        }
        let Some(inbound) = peer.inbound.as_mut().filter(|it| it.id == transfer) else {
            return Ok(());
        };
        let first_time = inbound.bitmap.insert(index);
        if let Some(handle) = peer.lanes.get_mut(&lane) {
            handle
                .meter
                .record_received_chunk(payload_bytes, !first_time);
            // Acknowledge on the lane that carried it, naming that lane, so the sender can
            // attribute the delivery even if this frame takes a different route back.
            let ack = ChunkAck::new(
                transfer,
                vec![Delivery {
                    index,
                    lane,
                    arrived_at_micros: u32::try_from(arrived_at_micros).unwrap_or(u32::MAX),
                }],
            );
            handle.send_priority(Outbound::Frame {
                kind: FrameKind::ChunkAck,
                body: ack.to_bytes()?,
            });
        }
        if inbound.bitmap.is_complete() {
            let report = Self::build_report(
                peer,
                TransferRole::Receiving,
                self.config.clock.now_micros(),
                &self.config,
                self.pool.starvations(),
            );
            peer.inbound = None;
            *sink_slot(&peer.sink) = None;
            let _ = peer.session.signal(SessionSignal::TransferFinished);
            let _ = events.send(EngineEvent::TransferFinished {
                device,
                outcome: TransferOutcome::Completed,
            });
            let _ = events.send(EngineEvent::TransferReport {
                device,
                report: Box::new(report),
            });
        }
        Ok(())
    }

    fn on_control(
        &mut self,
        device: DeviceId,
        _lane: LaneId,
        message: ControlMessage,
    ) -> Result<(), EngineError> {
        match message {
            ControlMessage::TransferOffer { transfer, manifest } => {
                self.on_offer(device, transfer, manifest)
            }
            ControlMessage::TransferAccept { transfer, .. } => self.on_accepted(device, transfer),
            ControlMessage::TransferReady { transfer } => {
                if let Some(peer) = self.peers.get_mut(&device)
                    && let Some(outbound) = peer.outbound.as_mut()
                    && outbound.id == transfer
                {
                    outbound.ready = true;
                    peer.setup.end("chunk map", self.config.clock.now_micros());
                }
                Ok(())
            }
            ControlMessage::ChunkMap(page) => self.on_chunk_map(device, &page),
            ControlMessage::LaneInvite(invite) => {
                self.emit(EngineEvent::LaneInvited {
                    device,
                    lane: invite.lane,
                    medium: invite.medium,
                    endpoint: invite.endpoint,
                });
                Ok(())
            }
            ControlMessage::BenchmarkMark { label } => {
                let now = self.config.clock.now_micros();
                if let Some(peer) = self.peers.get_mut(&device) {
                    peer.setup.begin(label, now);
                }
                Ok(())
            }
            ControlMessage::TransferDecline { .. } | ControlMessage::TransferCancel { .. } => {
                let events = self.events.clone();
                if let Some(peer) = self.peers.get_mut(&device) {
                    peer.outbound = None;
                    peer.inbound = None;
                    *sink_slot(&peer.sink) = None;
                }
                let _ = events.send(EngineEvent::TransferFinished {
                    device,
                    outcome: TransferOutcome::Cancelled,
                });
                Ok(())
            }
        }
    }

    fn on_offer(
        &mut self,
        device: DeviceId,
        transfer: TransferId,
        manifest: Manifest,
    ) -> Result<(), EngineError> {
        let now = self.config.clock.now_micros();
        let entry = manifest
            .entries()
            .first()
            .ok_or(EngineError::NoTransfer(device))?;
        let layout = entry.layout()?;
        let path = self.config.destination.join(entry.name());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::options()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)?;
        reserve(&file, entry.len())?;
        if !supports_positional_write(&file) {
            self.emit(EngineEvent::Diagnostic(
                Diagnostic::PositionalWritesUnavailable {
                    entry: entry.name().to_owned(),
                },
            ));
        }

        let events = self.events.clone();
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        peer.inbound = Some(InboundTransfer {
            id: transfer,
            layout,
            bitmap: ChunkBitmap::new(layout.count()),
            name: entry.name().to_owned(),
            root: *entry.root(),
            chaining_values: Vec::new(),
            pages: std::collections::BTreeSet::new(),
            destination: Arc::new(file),
            started_micros: now,
            ready: false,
        });
        peer.setup.begin("chunk map", now);
        let _ = events.send(EngineEvent::TransferOffered {
            device,
            transfer,
            manifest,
        });
        // A single-chunk payload carries no tree, so there is nothing to wait for.
        Self::finish_tree_if_complete(peer, now)
    }

    /// The peer agreed. Send it the verification tree before any chunk.
    fn on_accepted(&mut self, device: DeviceId, transfer: TransferId) -> Result<(), EngineError> {
        let now = self.config.clock.now_micros();
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let Some(outbound) = peer.outbound.as_mut().filter(|it| it.id == transfer) else {
            return Ok(());
        };
        outbound.accepted = true;
        peer.setup.end("offer", now);
        peer.setup.begin("chunk map", now);
        let pages = ChunkMapPage::paginate(transfer, &outbound.chaining_values);
        for page in pages {
            Self::send_control(peer, &ControlMessage::ChunkMap(page))?;
        }
        Ok(())
    }

    fn on_chunk_map(&mut self, device: DeviceId, page: &ChunkMapPage) -> Result<(), EngineError> {
        let now = self.config.clock.now_micros();
        let peer = self
            .peers
            .get_mut(&device)
            .ok_or(EngineError::UnknownPeer(device))?;
        let Some(inbound) = peer.inbound.as_mut().filter(|it| it.id == page.transfer) else {
            return Ok(());
        };
        inbound.absorb_page(page.first.as_u32(), &page.chaining_values);
        Self::finish_tree_if_complete(peer, now)
    }

    /// Builds the verifier once the whole tree is in, and tells the sender it may start.
    ///
    /// A partial tree verifies nothing — `ChunkVerifier::new` folds every value against the root
    /// — so chunks arriving before this point would be dropped, stall, and be sent again.
    fn finish_tree_if_complete(peer: &mut PeerState, now: u64) -> Result<(), EngineError> {
        let Some(inbound) = peer.inbound.as_mut() else {
            return Ok(());
        };
        if inbound.ready || !inbound.tree_is_complete() {
            return Ok(());
        }
        let verifier = ChunkVerifier::new(
            inbound.root,
            inbound.layout,
            inbound.chaining_values.clone(),
        )?;
        inbound.ready = true;
        let transfer = inbound.id;
        *sink_slot(&peer.sink) = Some(Arc::new(InboundSink {
            transfer,
            verifier: Arc::new(verifier),
            destination: Arc::clone(&inbound.destination),
            started_micros: now,
        }));
        peer.setup.end("chunk map", now);
        let _ = peer.session.signal(SessionSignal::TransferStarted);
        Self::send_control(peer, &ControlMessage::TransferReady { transfer })
    }

    fn on_ack(&mut self, device: DeviceId, ack: &ChunkAck) -> Result<(), EngineError> {
        let now = self.config.clock.now_millis();
        let events = self.events.clone();
        let Some(peer) = self.peers.get_mut(&device) else {
            return Ok(());
        };
        let Some(outbound) = peer.outbound.as_mut().filter(|it| it.id == ack.transfer) else {
            return Ok(());
        };
        for delivery in &ack.deliveries {
            outbound
                .scheduler
                .on_delivered(delivery.index, delivery.lane, now);
        }
        if outbound.scheduler.is_complete() {
            let report = Self::build_report(
                peer,
                TransferRole::Sending,
                self.config.clock.now_micros(),
                &self.config,
                self.pool.starvations(),
            );
            peer.outbound = None;
            let _ = peer.session.signal(SessionSignal::TransferFinished);
            let _ = events.send(EngineEvent::TransferFinished {
                device,
                outcome: TransferOutcome::Completed,
            });
            let _ = events.send(EngineEvent::TransferReport {
                device,
                report: Box::new(report),
            });
        }
        Ok(())
    }

    fn on_report(
        &mut self,
        device: DeviceId,
        lane: LaneId,
        report: LaneReport,
    ) -> Result<(), EngineError> {
        let Some(peer) = self.peers.get_mut(&device) else {
            return Ok(());
        };
        if let Some(outbound) = peer.outbound.as_mut() {
            outbound
                .scheduler
                .observe_throughput(lane, report.delivered_bytes, 1);
        }
        Ok(())
    }

    fn on_keepalive(
        &mut self,
        device: DeviceId,
        lane: LaneId,
        keepalive: KeepAlive,
        at_micros: u64,
    ) -> Result<(), EngineError> {
        let Some(peer) = self.peers.get_mut(&device) else {
            return Ok(());
        };
        let Some(handle) = peer.lanes.get_mut(&lane) else {
            return Ok(());
        };
        if !keepalive.is_echo {
            handle.send_priority(Outbound::Frame {
                kind: FrameKind::KeepAlive,
                body: keepalive.echo().to_bytes().to_vec(),
            });
            return Ok(());
        }
        let Some((nonce, sent_micros)) = handle.pending_ping else {
            return Ok(());
        };
        if nonce != keepalive.nonce {
            return Ok(());
        }
        handle.pending_ping = None;
        let rtt = u32::try_from(at_micros.saturating_sub(sent_micros)).unwrap_or(u32::MAX);
        handle.meter.observe_rtt(rtt);
        if let Some(outbound) = peer.outbound.as_mut() {
            outbound.scheduler.observe_rtt(lane, rtt);
        }
        Ok(())
    }

    fn on_lane_closed(
        &mut self,
        device: DeviceId,
        lane: LaneId,
        reason: Option<String>,
    ) -> Result<(), EngineError> {
        let now = self.config.clock.now_micros();
        let events = self.events.clone();
        let Some(peer) = self.peers.get_mut(&device) else {
            return Ok(());
        };
        if let Some(mut handle) = peer.lanes.remove(&lane) {
            handle.meter.close(now);
        }
        peer.session.release_lane(lane);
        if let Some(outbound) = peer.outbound.as_mut() {
            outbound.scheduler.remove_lane(lane);
        }
        if peer.lanes.is_empty() {
            let _ = peer.session.signal(SessionSignal::AllLanesLost);
        }
        let _ = events.send(EngineEvent::LaneClosed { device, lane });
        if let Some(reason) = reason {
            let _ = events.send(EngineEvent::Diagnostic(Diagnostic::LaneFailed {
                medium: Medium::Loopback,
                endpoint: String::new(),
                reason,
            }));
        }
        Ok(())
    }

    // --- MARK: writer notices ---

    fn on_notice(&mut self, notice: LaneNotice) {
        match notice {
            LaneNotice::Wrote {
                device,
                lane,
                frame_bytes,
                elapsed_micros,
                chunk,
            } => {
                if let Some(peer) = self.peers.get_mut(&device) {
                    if let Some(handle) = peer.lanes.get_mut(&lane) {
                        handle.meter.record_sent_frame(frame_bytes);
                        if chunk.is_some() {
                            handle.meter.record_sent_chunk();
                        }
                    }
                    if chunk.is_some()
                        && let Some(outbound) = peer.outbound.as_mut()
                    {
                        // Measured at the wire, at a resolution the scheduler's millisecond
                        // clock cannot reach.
                        outbound
                            .scheduler
                            .observe_throughput(lane, frame_bytes, elapsed_micros);
                    }
                }
            }
            LaneNotice::Abandoned {
                device,
                lane,
                index,
            } => {
                if let Some(peer) = self.peers.get_mut(&device)
                    && let Some(outbound) = peer.outbound.as_mut()
                {
                    outbound.scheduler.abandon(index, lane);
                    outbound.abandoned = outbound.abandoned.saturating_add(1);
                }
            }
            LaneNotice::Failed {
                device,
                lane,
                reason,
            } => {
                let _ = self.on_lane_closed(device, lane, Some(reason));
            }
        }
    }

    fn on_loaded(&mut self, loaded: Loaded) {
        let Loaded {
            peer: device,
            lane,
            index,
            buffer,
            error,
        } = loaded;
        let Some(peer) = self.peers.get_mut(&device) else {
            return;
        };
        if let Some(outbound) = peer.outbound.as_mut() {
            outbound.loading = outbound.loading.saturating_sub(1);
        }
        let Some(buffer) = buffer else {
            if let Some(reason) = error {
                self.emit(EngineEvent::Diagnostic(Diagnostic::EngineFailed { reason }));
            }
            return;
        };
        match peer.lanes.get(&lane) {
            Some(handle) => {
                handle.send_bulk(Outbound::Chunk { index, buffer });
            }
            None => self.pool.release(buffer),
        }
    }
}

impl Runtime {
    // --- MARK: the send pump ---

    /// Fills every lane that has room, for every transfer that is running.
    ///
    /// A buffer is taken before an assignment, not after: the scheduler records a chunk as in
    /// flight the moment it hands it out, and there is no way to give one back.
    fn pump(&mut self) {
        let now_millis = self.config.clock.now_millis();
        let devices: Vec<DeviceId> = self.peers.keys().copied().collect();
        for device in devices {
            let Some(peer) = self.peers.get_mut(&device) else {
                continue;
            };
            let Some(outbound) = peer.outbound.as_mut() else {
                continue;
            };
            if !outbound.ready {
                continue;
            }
            for index in outbound.scheduler.detect_stalls(now_millis) {
                for handle in peer.lanes.values() {
                    handle.send_priority(Outbound::Abandon(index));
                }
            }
            while let Some(buffer) = self.pool.take() {
                let Some(assignment) = outbound.scheduler.next_assignment(now_millis) else {
                    self.pool.release(buffer);
                    break;
                };
                outbound.loading += 1;
                spawn_read(
                    device,
                    assignment,
                    buffer,
                    Arc::clone(&outbound.source),
                    outbound.layout,
                    self.loaded.0.clone(),
                );
            }
        }
    }

    // --- MARK: the tick ---

    fn on_tick(&mut self) {
        let now_micros = self.config.clock.now_micros();
        let keepalive_gap = self.config.keepalive_interval_millis.saturating_mul(1_000);
        let events = self.events.clone();

        for (device, peer) in &mut self.peers {
            for handle in peer.lanes.values_mut() {
                if handle.pending_ping.is_none()
                    && now_micros.saturating_sub(handle.last_ping_micros) >= keepalive_gap
                {
                    let nonce = handle.next_nonce;
                    handle.next_nonce = handle.next_nonce.wrapping_add(1);
                    if handle.send_priority(Outbound::Frame {
                        kind: FrameKind::KeepAlive,
                        body: KeepAlive::ping(nonce).to_bytes().to_vec(),
                    }) {
                        handle.pending_ping = Some((nonce, now_micros));
                        handle.last_ping_micros = now_micros;
                    }
                }
            }

            let running = peer.inbound.is_some() || peer.outbound.is_some();
            if !running {
                continue;
            }
            let role = if peer.outbound.is_some() {
                TransferRole::Sending
            } else {
                TransferRole::Receiving
            };
            let mut lanes = Vec::with_capacity(peer.lanes.len());
            for handle in peer.lanes.values_mut() {
                let sample = handle.meter.sample(now_micros, role);
                peer.timeline.push(sample);
                lanes.push(LaneProgress {
                    lane: sample.lane,
                    medium: sample.medium,
                    delivered_bytes: sample.cumulative_bytes,
                    bandwidth_kbps: sample.kbps,
                });
            }
            let progress = peer
                .outbound
                .as_ref()
                .map(|outbound| outbound.scheduler.progress())
                .or_else(|| {
                    peer.inbound
                        .as_ref()
                        .map(|inbound| crate::transfer::Progress {
                            completed_chunks: inbound.bitmap.completed(),
                            total_chunks: inbound.bitmap.count(),
                            delivered_bytes: u64::from(inbound.bitmap.completed())
                                * u64::from(CHUNK_LEN),
                            total_bytes: inbound.layout.total_len(),
                        })
                });
            if let Some(progress) = progress {
                let _ = events.send(EngineEvent::TransferProgress {
                    device: *device,
                    progress,
                    lanes,
                });
            }
        }
        self.last_sample_micros = now_micros;
    }

    // --- MARK: reporting ---

    fn build_report(
        peer: &PeerState,
        role: TransferRole,
        now_micros: u64,
        config: &EngineConfig,
        starvations: u64,
    ) -> TransferReport {
        let (name, total_bytes, total_chunks, started, abandoned) = match role {
            TransferRole::Sending => peer
                .outbound
                .as_ref()
                .map(|it| {
                    (
                        it.name.clone(),
                        it.layout.total_len(),
                        it.layout.count(),
                        it.started_micros,
                        it.abandoned,
                    )
                })
                .unwrap_or_default(),
            TransferRole::Receiving => peer
                .inbound
                .as_ref()
                .map(|it| {
                    (
                        it.name.clone(),
                        it.layout.total_len(),
                        it.layout.count(),
                        it.started_micros,
                        0,
                    )
                })
                .unwrap_or_default(),
        };
        let stats: BTreeMap<LaneId, crate::transfer::LaneStats> = peer
            .outbound
            .as_ref()
            .map(|outbound| {
                outbound
                    .scheduler
                    .lane_stats()
                    .map(|stat| (stat.lane, stat))
                    .collect()
            })
            .unwrap_or_default();

        let lanes = peer
            .lanes
            .values()
            .map(|handle| {
                LaneSummary::new(
                    &handle.meter,
                    stats.get(&handle.meter.lane()),
                    now_micros,
                    role,
                )
            })
            .collect();

        TransferReport {
            role,
            name,
            total_bytes,
            total_chunks,
            wall_micros: now_micros.saturating_sub(started),
            conditions: RunConditions {
                max_bulk_lanes: config.tuning.max_bulk_lanes,
                max_inflight_per_lane: config.tuning.max_inflight_per_lane,
                buffer_pool_size: config.tuning.buffer_pool_size,
                buffer_footprint_bytes: config.tuning.buffer_footprint(),
                probing: config.tuning.probing,
                cipher_suite: peer.session.cipher_suite(),
                aes_hardware: has_aes_hardware(),
                chunk_len: CHUNK_LEN,
            },
            lanes,
            setup: peer.setup.clone(),
            timeline: peer.timeline.clone(),
            buffer_starvations: starvations,
            abandoned_chunks: abandoned,
        }
    }
}

/// Reads one chunk off the disk without holding up the engine.
fn spawn_read(
    device: DeviceId,
    assignment: crate::transfer::Assignment,
    mut buffer: ChunkBuffer,
    source: Arc<File>,
    layout: ChunkLayout,
    loaded: mpsc::UnboundedSender<Loaded>,
) {
    tokio::task::spawn_blocking(move || {
        let outcome = (|| -> Result<(), EngineError> {
            let offset =
                layout
                    .byte_offset(assignment.index)
                    .ok_or(EngineError::ChunkOutOfRange {
                        index: assignment.index,
                    })?;
            buffer.clear();
            buffer.fill_from(&chunk_header_bytes(assignment.index))?;
            let len = assignment.len as usize;
            let spare = buffer.spare_capacity_mut();
            let capacity = spare.len();
            let slot = spare
                .get_mut(..len)
                .ok_or(EngineError::ChunkTooLong { len, capacity })?;
            source.read_exact_at(offset, slot)?;
            buffer.commit(len)?;
            Ok(())
        })();

        let message = match outcome {
            Ok(()) => Loaded {
                peer: device,
                lane: assignment.lane,
                index: assignment.index,
                buffer: Some(buffer),
                error: None,
            },
            Err(error) => Loaded {
                peer: device,
                lane: assignment.lane,
                index: assignment.index,
                buffer: None,
                error: Some(error.to_string()),
            },
        };
        let _ = loaded.send(message);
    });
}
