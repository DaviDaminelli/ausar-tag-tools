//! Text in tag string tables is 8-bit Windows-1252, not UTF-8: real tables
//! hold `0x85` (`…`), `0x92` (`’`) and `0xE9` (`é`) (format-notes "Tags").

use std::borrow::Cow;

/// Characters for bytes 0x80–0x9F, where Windows-1252 differs from Latin-1.
/// The five undefined bytes map to the C1 control of the same value.
const HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2C6}', '\u{2030}',
    '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}',
    '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}',
    '\u{17E}', '\u{178}',
];

/// Decodes Windows-1252 text. ASCII-only text (almost every string) is
/// borrowed, not copied.
pub fn decode_cp1252(bytes: &[u8]) -> Cow<'_, str> {
    if bytes.is_ascii() {
        return Cow::Borrowed(std::str::from_utf8(bytes).expect("ASCII is UTF-8"));
    }
    Cow::Owned(
        bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9F => HIGH[(b - 0x80) as usize],
                // ASCII and 0xA0–0xFF are the Unicode code points of the same value.
                _ => b as char,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_borrowed() {
        assert!(matches!(decode_cp1252(b"objects\\weapons"), Cow::Borrowed("objects\\weapons")));
    }

    #[test]
    fn windows_1252_characters() {
        assert_eq!(decode_cp1252(b"fleeing_\x85!!!!"), "fleeing_\u{2026}!!!!");
        assert_eq!(decode_cp1252(b"don\x92t"), "don\u{2019}t");
        assert_eq!(decode_cp1252(b"caf\xe9"), "caf\u{e9}");
        assert_eq!(decode_cp1252(b"\x80\x9f\xa0\xff"), "\u{20AC}\u{178}\u{a0}\u{ff}");
    }
}
