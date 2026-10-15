use std::fs::File;
use std::os::unix::fs::FileExt;

use crate::io::IoError;

/// Reading a payload at an arbitrary offset.
pub trait PositionalRead {
    fn read_exact_at(&self, offset: u64, into: &mut [u8]) -> Result<(), IoError>;
}

/// Writing a chunk at its own offset.
///
/// Positional writes are what make out-of-order arrival free on the receiving side: chunks land
/// wherever they belong with no seeking, no ordering and no reassembly buffer, and several may be
/// written concurrently.
pub trait PositionalWrite {
    fn write_all_at(&self, offset: u64, data: &[u8]) -> Result<(), IoError>;

    /// Some Android document providers hand back a pipe, where positional writes fail outright.
    fn supports_positional_write(&self) -> bool;
}

impl PositionalRead for File {
    fn read_exact_at(&self, offset: u64, into: &mut [u8]) -> Result<(), IoError> {
        FileExt::read_exact_at(self, into, offset).map_err(|error| {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                IoError::ShortRead {
                    offset,
                    read: 0,
                    expected: into.len(),
                }
            } else {
                IoError::Os(error)
            }
        })
    }
}

impl PositionalWrite for File {
    fn write_all_at(&self, offset: u64, data: &[u8]) -> Result<(), IoError> {
        FileExt::write_all_at(self, data, offset).map_err(IoError::Os)
    }

    fn supports_positional_write(&self) -> bool {
        crate::io::supports_positional_write(self)
    }
}
