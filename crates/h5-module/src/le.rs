//! Little-endian reads from fixed-size records whose length was checked by the caller.

pub(crate) fn u16_at(rec: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(rec[off..off + 2].try_into().unwrap())
}

pub(crate) fn u32_at(rec: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(rec[off..off + 4].try_into().unwrap())
}

pub(crate) fn i32_at(rec: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(rec[off..off + 4].try_into().unwrap())
}

pub(crate) fn u64_at(rec: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(rec[off..off + 8].try_into().unwrap())
}

pub(crate) fn i64_at(rec: &[u8], off: usize) -> i64 {
    i64::from_le_bytes(rec[off..off + 8].try_into().unwrap())
}
