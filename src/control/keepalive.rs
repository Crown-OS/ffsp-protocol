use crate::control::ControlError;
use crate::control::manifest_codec::truncated;
use crate::util::cursor::ByteReader;

/// A round trip on one lane, and nothing else.
///
/// Latency per medium is a headline benchmark figure, and it cannot be inferred from chunk
/// timings: a 1 MiB chunk on Bluetooth measures throughput, not latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAlive {
    pub nonce: u64,
    /// Set on the reply, so the sender knows which of the two it is holding.
    pub is_echo: bool,
}

impl KeepAlive {
    pub const LEN: usize = 9;

    pub const fn ping(nonce: u64) -> Self {
        Self {
            nonce,
            is_echo: false,
        }
    }

    pub const fn echo(self) -> Self {
        Self {
            nonce: self.nonce,
            is_echo: true,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        if let Some(slot) = out.get_mut(..8) {
            slot.copy_from_slice(&self.nonce.to_le_bytes());
        }
        if let Some(slot) = out.get_mut(8) {
            *slot = u8::from(self.is_echo);
        }
        out
    }

    pub fn parse(source: &[u8]) -> Result<Self, ControlError> {
        let mut reader = ByteReader::new(source);
        let keepalive = Self {
            nonce: reader.u64().ok_or(truncated(&reader))?,
            is_echo: reader.u8().ok_or(truncated(&reader))? != 0,
        };
        if !reader.is_exhausted() {
            return Err(ControlError::TrailingBytes {
                trailing: reader.remaining(),
            });
        }
        Ok(keepalive)
    }
}
