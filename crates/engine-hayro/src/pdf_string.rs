//! PDF text strings.

/// A PDF text string with its ends trimmed.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    decode_untrimmed(bytes).trim().to_owned()
}

/// A PDF text string: UTF-16BE or UTF-8 with a byte-order mark, else
/// PDFDocEncoding.
pub(crate) fn decode_untrimmed(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = utf16
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_be_bytes(pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else if let Some(utf8) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(utf8).into_owned()
    } else {
        bytes.iter().map(|&b| pdf_doc_char(b)).collect()
    }
}

/// PDFDocEncoding: Latin-1 but for 0x80–0xA0.
fn pdf_doc_char(byte: u8) -> char {
    const HIGH: [char; 33] = [
        '•', '†', '‡', '…', '—', '–', 'ƒ', '⁄', '‹', '›', '−', '‰', '„', '“', '”', '‘', '’', '‚',
        '™', 'ﬁ', 'ﬂ', 'Ł', 'Œ', 'Š', 'Ÿ', 'Ž', 'ı', 'ł', 'œ', 'š', 'ž', '\u{FFFD}', '€',
    ];
    match byte {
        0x80..=0xA0 => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}
