use blake3::hazmat::ChainingValue;

use crate::control::TransferId;
use crate::control::error::ControlError;
use crate::control::manifest_codec::truncated;
use crate::util::cursor::ByteReader;
use crate::wire::ChunkIndex;

/// How many chaining values travel in one page.
///
/// The whole vector is 32 bytes per chunk, so a 64 GiB payload carries 2 MiB of it. Paging keeps
/// the receiver verifying from the first chunk rather than after the last byte of metadata, and
/// keeps the page inside what Bluetooth can carry while it is still the only lane up.
pub const CHUNK_MAP_PAGE_LEN: u32 = 256;

/// One page of the chunk verification tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMapPage {
    pub transfer: TransferId,
    pub first: ChunkIndex,
    pub chaining_values: Vec<ChainingValue>,
}

impl ChunkMapPage {
    /// Splits a whole chaining-value vector into the pages that carry it.
    pub fn paginate(transfer: TransferId, chaining_values: &[ChainingValue]) -> Vec<Self> {
        chaining_values
            .chunks(CHUNK_MAP_PAGE_LEN as usize)
            .enumerate()
            .map(|(page, values)| Self {
                transfer,
                first: ChunkIndex::new(
                    u32::try_from(page)
                        .unwrap_or(u32::MAX)
                        .saturating_mul(CHUNK_MAP_PAGE_LEN),
                ),
                chaining_values: values.to_vec(),
            })
            .collect()
    }

    pub(crate) fn encode(&self, out: &mut Vec<u8>) -> Result<(), ControlError> {
        let count =
            u32::try_from(self.chaining_values.len()).map_err(|_| ControlError::FieldTooLong)?;
        out.extend_from_slice(&self.transfer.as_u32().to_le_bytes());
        out.extend_from_slice(&self.first.as_u32().to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        for value in &self.chaining_values {
            out.extend_from_slice(value);
        }
        Ok(())
    }

    pub(crate) fn decode(reader: &mut ByteReader<'_>) -> Result<Self, ControlError> {
        let transfer = TransferId::new(reader.u32().ok_or(truncated(reader))?);
        let first = ChunkIndex::new(reader.u32().ok_or(truncated(reader))?);
        let count = reader.u32().ok_or(truncated(reader))?;
        let mut chaining_values = Vec::with_capacity(count as usize);
        for _ in 0..count {
            chaining_values.push(reader.array::<32>().ok_or(truncated(reader))?);
        }
        Ok(Self {
            transfer,
            first,
            chaining_values,
        })
    }
}
