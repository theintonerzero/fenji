/// Formats `bytes` as a 16-bytes-per-line hex dump. `base_offset` is the stream offset of
/// `bytes[0]`, so consecutive reads of one connection get continuous offsets.
///
/// ```text
/// 00000000  68 65 6c 6c 6f                                   |hello|
/// ```
pub fn hexdump(base_offset: usize, bytes: &[u8]) -> String {
    let mut out = String::new();
    for (line, chunk) in bytes.chunks(16).enumerate() {
        let offset = base_offset + line * 16;
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!("{offset:08x}  {:<47}  |{ascii}|\n", hex.join(" ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::hexdump;

    #[test]
    fn empty_input_produces_no_lines() {
        assert_eq!(hexdump(0, &[]), "");
    }

    #[test]
    fn short_line_is_padded_so_ascii_column_aligns() {
        assert_eq!(
            hexdump(0, b"hello"),
            "00000000  68 65 6c 6c 6f                                   |hello|\n"
        );
    }

    #[test]
    fn non_printable_bytes_show_as_dots() {
        assert_eq!(
            hexdump(0, &[0x00, 0x41, 0xff, 0x0a]),
            "00000000  00 41 ff 0a                                      |.A..|\n"
        );
    }

    #[test]
    fn splits_into_16_byte_lines_and_continues_offsets() {
        let bytes: Vec<u8> = (0u8..20).collect();
        let dump = hexdump(0x20, &bytes);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("00000020  00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f  |")
        );
        assert!(lines[1].starts_with("00000030  10 11 12 13 "));
    }
}
