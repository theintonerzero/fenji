//! Text the server passes on for others to see: chat lines, announcements and command arguments.

/// Characters that show nothing but change how the text around them shows: controls, the
/// bidirectional overrides and isolates, zero-width characters and the line separators. In a line
/// of chat or a command they could make it read as something else, in players' clients or in a
/// host's terminal.
pub fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
        )
}

/// Whether `text` has no [`hidden`] character.
pub fn plain(text: &str) -> bool {
    !text.chars().any(hidden)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_invisible_formatting_are_not_plain() {
        assert!(plain("Hello, Mai! 你好 é #12"));
        for bad in [
            "a\u{0}b",
            "line\nbreak",
            "\u{1b}[31mred",
            "\u{9b}31m",
            "\u{202E}txet",
            "\u{2066}isolate\u{2069}",
            "zero\u{200B}width",
            "joiner\u{200D}",
            "\u{2028}separator",
            "\u{FEFF}bom",
            "\u{061C}mark",
        ] {
            assert!(!plain(bad), "{bad:?}");
        }
    }
}
