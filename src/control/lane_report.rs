use crate::control::ControlError;
use crate::control::manifest_codec::truncated;
use crate::util::cursor::ByteReader;

/// What one end has observed of a lane, sent to the other.
///
/// The receiver is the only side that knows when bytes actually arrived; the sender only learns
/// of an arrival when the acknowledgement returns. Sending the receiver's own figures across is
/// what lets a report distinguish a slow medium from a slow acknowledgement path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaneReport {
    pub bandwidth_kbps: u32,
    pub rtt_micros: u32,
    pub delivered_bytes: u64,
    pub delivered_chunks: u32,
}

impl LaneReport {
    pub const LEN: usize = 20;

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        let fields = [
            self.bandwidth_kbps.to_le_bytes(),
            self.rtt_micros.to_le_bytes(),
        ];
        let mut at = 0;
        for field in fields {
            if let Some(slot) = out.get_mut(at..at + 4) {
                slot.copy_from_slice(&field);
            }
            at += 4;
        }
        if let Some(slot) = out.get_mut(8..16) {
            slot.copy_from_slice(&self.delivered_bytes.to_le_bytes());
        }
        if let Some(slot) = out.get_mut(16..20) {
            slot.copy_from_slice(&self.delivered_chunks.to_le_bytes());
        }
        out
    }

    pub fn parse(source: &[u8]) -> Result<Self, ControlError> {
        let mut reader = ByteReader::new(source);
        let report = Self {
            bandwidth_kbps: reader.u32().ok_or(truncated(&reader))?,
            rtt_micros: reader.u32().ok_or(truncated(&reader))?,
            delivered_bytes: reader.u64().ok_or(truncated(&reader))?,
            delivered_chunks: reader.u32().ok_or(truncated(&reader))?,
        };
        if !reader.is_exhausted() {
            return Err(ControlError::TrailingBytes {
                trailing: reader.remaining(),
            });
        }
        Ok(report)
    }
}
