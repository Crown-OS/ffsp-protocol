use crate::control::chunk_map::ChunkMapPage;
use crate::control::error::ControlError;
use crate::control::invite::LaneInvite;
use crate::control::manifest_codec::{encode_manifest, read_manifest, truncated};
use crate::control::opcode::ControlOpcode;
use crate::control::reason::EndReason;
use crate::control::transfer_id::TransferId;
use crate::transfer::{Manifest, ResumeState};
use crate::util::cursor::{ByteReader, write_string};

/// Everything the engine says to its peer that is not a chunk.
///
/// The engine interprets all of these. A [`FrameKind::Control`](crate::wire::FrameKind) body
/// whose opcode falls outside this set is passed to the platform untouched as
/// [`EngineEvent::ControlFrame`](crate::engine::EngineEvent), which is what that event was always
/// for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlMessage {
    /// What is on offer, sent before any chunk moves.
    TransferOffer {
        transfer: TransferId,
        manifest: Manifest,
    },
    /// Agreement, and whatever of it the receiver already holds.
    TransferAccept {
        transfer: TransferId,
        resume: Option<ResumeState>,
    },
    TransferDecline {
        transfer: TransferId,
        reason: EndReason,
    },
    TransferCancel {
        transfer: TransferId,
        reason: EndReason,
    },
    /// One page of the verification tree.
    ChunkMap(ChunkMapPage),
    /// Bring up another medium and join it to this session.
    LaneInvite(LaneInvite),
    /// Brackets a phase so both timelines can be lined up afterwards.
    BenchmarkMark { label: String },
    /// The receiver has the whole verification tree and can verify a chunk the moment it lands.
    ///
    /// [`ChunkVerifier::new`](crate::transfer::ChunkVerifier::new) folds every chaining value
    /// against the root, so a partial tree verifies nothing. Without this the first chunks would
    /// race the last page of the map and be dropped, then stall, then be resent.
    TransferReady { transfer: TransferId },
}

impl ControlMessage {
    /// Opcodes at or above this are the platform's own vocabulary, not the engine's.
    pub const PLATFORM_OPCODE_BASE: u8 = 0x80;

    pub const fn opcode(&self) -> ControlOpcode {
        match self {
            Self::TransferOffer { .. } => ControlOpcode::TransferOffer,
            Self::TransferAccept { .. } => ControlOpcode::TransferAccept,
            Self::TransferDecline { .. } => ControlOpcode::TransferDecline,
            Self::TransferCancel { .. } => ControlOpcode::TransferCancel,
            Self::ChunkMap(_) => ControlOpcode::ChunkMap,
            Self::LaneInvite(_) => ControlOpcode::LaneInvite,
            Self::BenchmarkMark { .. } => ControlOpcode::BenchmarkMark,
            Self::TransferReady { .. } => ControlOpcode::TransferReady,
        }
    }

    /// Whether a control body belongs to the engine or should be handed to the platform.
    pub const fn is_engine_opcode(first_byte: u8) -> bool {
        first_byte < Self::PLATFORM_OPCODE_BASE
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ControlError> {
        let mut out = vec![self.opcode().as_byte()];
        match self {
            Self::TransferOffer { transfer, manifest } => {
                out.extend_from_slice(&transfer.as_u32().to_le_bytes());
                encode_manifest(manifest, &mut out)?;
            }
            Self::TransferAccept { transfer, resume } => {
                out.extend_from_slice(&transfer.as_u32().to_le_bytes());
                match resume {
                    Some(state) => {
                        out.push(1);
                        out.extend_from_slice(&state.to_bytes());
                    }
                    None => out.push(0),
                }
            }
            Self::TransferDecline { transfer, reason }
            | Self::TransferCancel { transfer, reason } => {
                out.extend_from_slice(&transfer.as_u32().to_le_bytes());
                out.push(reason.as_byte());
            }
            Self::ChunkMap(page) => page.encode(&mut out)?,
            Self::LaneInvite(invite) => invite.encode(&mut out)?,
            Self::BenchmarkMark { label } => {
                write_string(&mut out, label).ok_or(ControlError::FieldTooLong)?;
            }
            Self::TransferReady { transfer } => {
                out.extend_from_slice(&transfer.as_u32().to_le_bytes());
            }
        }
        Ok(out)
    }

    pub fn parse(source: &[u8]) -> Result<Self, ControlError> {
        let mut reader = ByteReader::new(source);
        let opcode = ControlOpcode::from_byte(reader.u8().ok_or(truncated(&reader))?)?;
        let message = match opcode {
            ControlOpcode::TransferOffer => Self::TransferOffer {
                transfer: TransferId::new(reader.u32().ok_or(truncated(&reader))?),
                manifest: read_manifest(&mut reader)?,
            },
            ControlOpcode::TransferAccept => {
                let transfer = TransferId::new(reader.u32().ok_or(truncated(&reader))?);
                let resume = match reader.u8().ok_or(truncated(&reader))? {
                    0 => None,
                    _ => {
                        let rest = reader.take(reader.remaining()).ok_or(truncated(&reader))?;
                        Some(ResumeState::parse(rest)?)
                    }
                };
                Self::TransferAccept { transfer, resume }
            }
            ControlOpcode::TransferDecline => Self::TransferDecline {
                transfer: TransferId::new(reader.u32().ok_or(truncated(&reader))?),
                reason: EndReason::from_byte(reader.u8().ok_or(truncated(&reader))?)?,
            },
            ControlOpcode::TransferCancel => Self::TransferCancel {
                transfer: TransferId::new(reader.u32().ok_or(truncated(&reader))?),
                reason: EndReason::from_byte(reader.u8().ok_or(truncated(&reader))?)?,
            },
            ControlOpcode::ChunkMap => Self::ChunkMap(ChunkMapPage::decode(&mut reader)?),
            ControlOpcode::LaneInvite => Self::LaneInvite(LaneInvite::decode(&mut reader)?),
            ControlOpcode::BenchmarkMark => Self::BenchmarkMark {
                label: reader.string().ok_or(truncated(&reader))?,
            },
            ControlOpcode::TransferReady => Self::TransferReady {
                transfer: TransferId::new(reader.u32().ok_or(truncated(&reader))?),
            },
        };
        if !reader.is_exhausted() {
            return Err(ControlError::TrailingBytes {
                trailing: reader.remaining(),
            });
        }
        Ok(message)
    }
}
