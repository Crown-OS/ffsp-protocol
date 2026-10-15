use std::fs::File;

/// Whether a descriptor accepts writes at an offset.
///
/// Worth checking before a transfer starts rather than discovering it on the first chunk: a
/// document provider that returns a pipe fails every positional write, and the whole
/// out-of-order write design depends on this being true.
pub fn supports_positional_write(file: &File) -> bool {
    file.metadata()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

/// Reserves space for the destination up front.
///
/// A sparse allocation, since the standard library exposes no `fallocate`. Writes at arbitrary
/// offsets still succeed; only the up-front space guarantee is missing.
pub fn reserve(file: &File, len: u64) -> Result<(), crate::io::IoError> {
    file.set_len(len).map_err(crate::io::IoError::Os)
}
