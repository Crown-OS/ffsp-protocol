pub(crate) fn read_u16_le(source: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let field: [u8; 2] = source.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(field))
}

pub(crate) fn read_u24_le(source: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(3)?;
    let [low, mid, high]: [u8; 3] = source.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes([low, mid, high, 0]))
}

pub(crate) fn read_u32_le(source: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let field: [u8; 4] = source.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(field))
}

pub(crate) const fn u24_le(value: u32) -> [u8; 3] {
    let [low, mid, high, _] = value.to_le_bytes();
    [low, mid, high]
}

pub(crate) fn read_u64_le(source: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let field: [u8; 8] = source.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(field))
}
