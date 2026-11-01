use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A tag's global ID: the same in every module (PRD §4). Stored as an `i32`;
/// `-1` means "not a tag".
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GlobalId(pub i32);

impl GlobalId {
    /// The value stored for file entries that aren't tags.
    pub const NONE: GlobalId = GlobalId(-1);

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }
}

/// Written as `0x` + 8 uppercase hex digits: `0x00A1B2C3`.
impl fmt::Display for GlobalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:08X}", self.0 as u32)
    }
}

impl fmt::Debug for GlobalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlobalId({self})")
    }
}

/// Parses `0x` + 1 to 8 hex digits, case-insensitive.
impl FromStr for GlobalId {
    type Err = String;

    fn from_str(s: &str) -> Result<GlobalId, String> {
        let invalid = || format!("invalid global ID '{s}' (global IDs look like 0x00A1B2C3)");
        let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).ok_or_else(invalid)?;
        if hex.is_empty() || hex.len() > 8 {
            return Err(invalid());
        }
        u32::from_str_radix(hex, 16).map(|v| GlobalId(v as i32)).map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_as_8_uppercase_hex_digits() {
        assert_eq!(GlobalId(0x00A1_B2C3).to_string(), "0x00A1B2C3");
        assert_eq!(GlobalId(-1).to_string(), "0xFFFFFFFF");
        assert_eq!(GlobalId(0xD101_8FAEu32 as i32).to_string(), "0xD1018FAE");
    }

    #[test]
    fn parses_hex_with_prefix() {
        assert_eq!("0x00a1b2c3".parse(), Ok(GlobalId(0x00A1_B2C3)));
        assert_eq!("0X10".parse(), Ok(GlobalId(0x10)));
        assert_eq!("0xD1018FAE".parse(), Ok(GlobalId(0xD101_8FAEu32 as i32)));
    }

    #[test]
    fn rejects_other_text() {
        for s in ["", "0x", "123", "0xZZ", "0x123456789", "objects\\x"] {
            assert!(s.parse::<GlobalId>().is_err(), "{s}");
        }
    }

    #[test]
    fn none_is_minus_one() {
        assert!(GlobalId(-1).is_none());
        assert!(!GlobalId(0).is_none());
    }
}
