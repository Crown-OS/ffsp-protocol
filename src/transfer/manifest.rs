use blake3::Hash;

use crate::transfer::{ChunkLayout, TransferError};

/// One file offered in a transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    name: String,
    len: u64,
    root: Hash,
}

impl FileEntry {
    /// Rejects any name that could write outside the destination.
    ///
    /// The sender chooses these strings, so treating them as trusted paths would let a peer
    /// overwrite arbitrary files. Absolute paths, parent traversals and empty names are refused
    /// here rather than at the point of opening a file, where the check is easy to forget.
    pub fn new(name: String, len: u64, root: Hash) -> Result<Self, TransferError> {
        let is_safe = !name.is_empty()
            && !name.starts_with('/')
            && !name.starts_with('\\')
            && !name.contains(':')
            && !name.contains('\0')
            && name
                .split(['/', '\\'])
                .all(|part| !part.is_empty() && part != ".." && part != ".");
        if !is_safe {
            return Err(TransferError::UnsafeEntryName(name));
        }
        Ok(Self { name, len, root })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn len(&self) -> u64 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn root(&self) -> &Hash {
        &self.root
    }

    pub fn layout(&self) -> Result<ChunkLayout, TransferError> {
        ChunkLayout::new(self.len)
    }
}

/// Everything a transfer will move, agreed before any bytes flow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    entries: Vec<FileEntry>,
}

impl Manifest {
    pub const fn new(entries: Vec<FileEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    pub fn total_len(&self) -> u64 {
        self.entries.iter().map(FileEntry::len).sum()
    }

    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
