/// A forward-only reader over a frame body.
///
/// The codecs here are all "read a field, then the next"; tracking the offset by hand at every
/// call site is where the off-by-one lives.
#[derive(Debug)]
pub(crate) struct ByteReader<'a> {
    source: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) const fn new(source: &'a [u8]) -> Self {
        Self { source, offset: 0 }
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.offset.checked_add(len)?;
        let field = self.source.get(self.offset..end)?;
        self.offset = end;
        Some(field)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[byte]| byte)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array::<2>().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array::<4>().map(u32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.array::<8>().map(u64::from_le_bytes)
    }

    /// A length-prefixed UTF-8 string.
    pub(crate) fn string(&mut self) -> Option<String> {
        let len = usize::from(self.u16()?);
        let raw = self.take(len)?;
        core::str::from_utf8(raw).ok().map(str::to_owned)
    }

    pub(crate) const fn consumed(&self) -> usize {
        self.offset
    }

    pub(crate) const fn remaining(&self) -> usize {
        self.source.len() - self.offset
    }

    pub(crate) const fn is_exhausted(&self) -> bool {
        self.remaining() == 0
    }
}

/// Appends a length-prefixed UTF-8 string, refusing one too long to describe.
pub(crate) fn write_string(out: &mut Vec<u8>, value: &str) -> Option<()> {
    let len = u16::try_from(value.len()).ok()?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Some(())
}
