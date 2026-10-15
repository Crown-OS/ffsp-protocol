use crate::control::error::ControlError;
use crate::control::manifest_codec::truncated;
use crate::control::transfer_id::TransferId;
use crate::util::cursor::ByteReader;
use crate::wire::{ChunkIndex, LaneId};

/// One chunk the receiver verified and wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    pub index: ChunkIndex,
    /// The lane the chunk actually arrived on.
    ///
    /// Not the lane this acknowledgement travels over: those differ whenever acknowledgements
    /// are routed to the lowest-latency lane instead of the one carrying the bytes, and the
    /// sender attributes delivery by this field.
    pub lane: LaneId,
    /// Microseconds since the receiver started this transfer, by the receiver's own clock.
    ///
    /// A shared wall clock is not available and would not be trustworthy if it were, so the two
    /// timelines are each measured from their own start.
    pub arrived_at_micros: u32,
}

/// Chunks the receiver has verified and written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkAck {
    pub transfer: TransferId,
    pub deliveries: Vec<Delivery>,
}

impl ChunkAck {
    const ENTRY_LEN: usize = 10;

    pub const fn new(transfer: TransferId, deliveries: Vec<Delivery>) -> Self {
        Self {
            transfer,
            deliveries,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, ControlError> {
        let count = u16::try_from(self.deliveries.len()).map_err(|_| ControlError::FieldTooLong)?;
        let mut out = Vec::with_capacity(6 + self.deliveries.len() * Self::ENTRY_LEN);
        out.extend_from_slice(&self.transfer.as_u32().to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        for delivery in &self.deliveries {
            out.extend_from_slice(&delivery.index.as_u32().to_le_bytes());
            out.extend_from_slice(&delivery.lane.as_u16().to_le_bytes());
            out.extend_from_slice(&delivery.arrived_at_micros.to_le_bytes());
        }
        Ok(out)
    }

    pub fn parse(source: &[u8]) -> Result<Self, ControlError> {
        let mut reader = ByteReader::new(source);
        let transfer = TransferId::new(reader.u32().ok_or(truncated(&reader))?);
        let count = reader.u16().ok_or(truncated(&reader))?;
        let mut deliveries = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            deliveries.push(Delivery {
                index: ChunkIndex::new(reader.u32().ok_or(truncated(&reader))?),
                lane: LaneId::new(reader.u16().ok_or(truncated(&reader))?),
                arrived_at_micros: reader.u32().ok_or(truncated(&reader))?,
            });
        }
        if !reader.is_exhausted() {
            return Err(ControlError::TrailingBytes {
                trailing: reader.remaining(),
            });
        }
        Ok(Self {
            transfer,
            deliveries,
        })
    }
}
