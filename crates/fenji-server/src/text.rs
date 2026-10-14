//! Text the server passes on for others to see: chat lines, announcements and command arguments.

/// Characters that show nothing, or change how the text around them shows:
/// - controls, and the line and paragraph separators;
/// - every format character (Unicode's category Cf, as of Unicode 18.0), such as the
///   bidirectional marks, overrides and isolates, the zero-width characters and the tags;
/// - the Hangul fillers, which show as blanks.
///
/// In a line of chat or a command they could make it read as something else, in players' clients
/// or in a host's terminal, carry text no one sees, or make a line that looks empty. std has no
/// general-category lookup, so the format characters are listed here.
pub fn hidden(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{0600}'..='\u{0605}'
                | '\u{061C}'
                | '\u{06DD}'
                | '\u{070F}'
                | '\u{0890}'..='\u{0891}'
                | '\u{08E2}'
                | '\u{115F}'..='\u{1160}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{206A}'..='\u{206F}'
                | '\u{3164}'
                | '\u{FEFF}'
                | '\u{FFA0}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{110BD}'
                | '\u{110CD}'
                | '\u{13430}'..='\u{1343F}'
                | '\u{1BCA0}'..='\u{1BCA3}'
                | '\u{1D173}'..='\u{1D17A}'
                | '\u{E0001}'
                | '\u{E0020}'..='\u{E007F}'
        )
}

/// Whether `text` has no [`hidden`] character.
pub fn plain(text: &str) -> bool {
    !text.chars().any(hidden)
}

/// `text` with each [`hidden`] character written as its escape, such as `\u{1b}`, for a host's
/// terminal: text stored before the server checked it may hold them.
pub fn escape_hidden(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if hidden(c) {
            out.extend(c.escape_unicode());
        } else {
            out.push(c);
        }
    }
    out
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
            "soft\u{00AD}hyphen",
            "tag\u{E0041}",
            "\u{180E}vowel",
            "\u{3164}",
        ] {
            assert!(!plain(bad), "{bad:?}");
        }
    }
}
