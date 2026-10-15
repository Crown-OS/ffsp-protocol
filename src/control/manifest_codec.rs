use blake3::Hash;

use crate::control::ControlError;
use crate::transfer::{FileEntry, Manifest};
use crate::util::cursor::{ByteReader, write_string};

/// Encodes what is on offer.
///
/// [`FileEntry::new`] rejects a name that could escape the destination, so decoding here is also
/// where a hostile manifest stops.
pub fn encode_manifest(manifest: &Manifest, out: &mut Vec<u8>) -> Result<(), ControlError> {
    let count = u32::try_from(manifest.len()).map_err(|_| ControlError::FieldTooLong)?;
    out.extend_from_slice(&count.to_le_bytes());
    for entry in manifest.entries() {
        write_string(out, entry.name()).ok_or(ControlError::FieldTooLong)?;
        out.extend_from_slice(&entry.len().to_le_bytes());
        out.extend_from_slice(entry.root().as_bytes());
    }
    Ok(())
}

pub(crate) fn read_manifest(reader: &mut ByteReader<'_>) -> Result<Manifest, ControlError> {
    let count = reader.u32().ok_or(truncated(reader))?;
    let mut entries = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let name = reader.string().ok_or(truncated(reader))?;
        let len = reader.u64().ok_or(truncated(reader))?;
        let root: [u8; 32] = reader.array().ok_or(truncated(reader))?;
        entries.push(FileEntry::new(name, len, Hash::from_bytes(root))?);
    }
    Ok(Manifest::new(entries))
}

pub(crate) const fn truncated(reader: &ByteReader<'_>) -> ControlError {
    ControlError::Truncated {
        read: reader.consumed(),
    }
}
