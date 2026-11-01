use std::fmt;

use serde::{Deserialize, Serialize};

/// A four-character code (class code / group tag, module magic).
///
/// Stored in files as a little-endian `u32`; the characters read in order from
/// the most significant byte. So the on-disk bytes `70 73 62 73` (`psbs`) are
/// the code `sbsp` (format-notes: "Class code").
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FourCc(pub u32);

impl FourCc {
    /// The value stored for entries that aren't tags (`-1`).
    pub const NONE: FourCc = FourCc(u32::MAX);

    /// Builds a code from text. Codes shorter than 4 characters are padded with
    /// spaces (definition file names trim them: `(mat)` is `mat `).
    pub fn from_code(code: &str) -> Option<FourCc> {
        let bytes = code.as_bytes();
        if bytes.is_empty() || bytes.len() > 4 || !bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
            return None;
        }
        let mut buf = [b' '; 4];
        buf[..bytes.len()].copy_from_slice(bytes);
        Some(FourCc(u32::from_be_bytes(buf)))
    }

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    pub fn bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return f.write_str("-1");
        }
        for b in self.bytes() {
            if b.is_ascii_graphic() || b == b' ' {
                write!(f, "{}", b as char)?;
            } else {
                write!(f, "\\x{b:02X}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_disk_bytes_read_in_reverse() {
        // Bytes as stored in w1_meridian's sbsp entry.
        let stored = u32::from_le_bytes(*b"psbs");
        assert_eq!(FourCc(stored).to_string(), "sbsp");
        assert_eq!(FourCc::from_code("sbsp"), Some(FourCc(stored)));
    }

    #[test]
    fn magic_is_dhom() {
        assert_eq!(FourCc(u32::from_le_bytes(*b"mohd")).to_string(), "dhom");
    }

    #[test]
    fn short_codes_are_space_padded() {
        assert_eq!(FourCc::from_code("mat").unwrap().to_string(), "mat ");
        assert_eq!(FourCc::from_code("mat"), FourCc::from_code("mat "));
    }

    #[test]
    fn symbols_allowed() {
        for code in ["_fx_", "snd!", "sfx+", "hsc_"] {
            assert_eq!(FourCc::from_code(code).unwrap().to_string(), code);
        }
    }

    #[test]
    fn invalid_codes() {
        assert_eq!(FourCc::from_code(""), None);
        assert_eq!(FourCc::from_code("toolong"), None);
        assert_eq!(FourCc::from_code("a\tb"), None);
    }

    #[test]
    fn none_displays_as_minus_one() {
        assert_eq!(FourCc::NONE.to_string(), "-1");
    }
}
