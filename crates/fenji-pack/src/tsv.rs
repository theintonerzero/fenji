//! A pack's text files: one entry a line, `<fingerprint>` TAB `<English>`, sorted by fingerprint,
//! with tabs, line breaks and backslashes escaped (retranslation spec §6.2).
use crate::{Fingerprint, PackError};

/// One line's new English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub fingerprint: Fingerprint,
    pub english: String,
}

/// `text` with its backslashes, tabs and line breaks escaped, so that it fits one field.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '\t' => out.push_str(r"\t"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            c => out.push(c),
        }
    }
    out
}

/// A field's text: only `\\`, `\t`, `\n` and `\r` escapes, and no raw tab or line break.
pub fn unescape(field: &str) -> Result<String, PackError> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('\\') => out.push('\\'),
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some(other) => return Err(PackError::Escape(format!("\\{other}"))),
                None => return Err(PackError::Escape("\\".into())),
            },
            '\t' | '\n' | '\r' => return Err(PackError::Escape(c.escape_default().to_string())),
            c => out.push(c),
        }
    }
    Ok(out)
}

/// `entries` as a file: sorted by fingerprint, one line each.
pub fn write_tsv(entries: &mut [Entry]) -> String {
    entries.sort_by_key(|e| e.fingerprint);
    let mut out = String::new();
    for e in entries.iter() {
        out.push_str(&e.fingerprint.to_string());
        out.push('\t');
        out.push_str(&escape(&e.english));
        out.push('\n');
    }
    out
}

/// A file's entries, refused unless every line is an entry and they're in strictly increasing
/// fingerprint order, so that no fingerprint appears twice.
pub fn read_tsv(text: &str) -> Result<Vec<Entry>, PackError> {
    let mut entries: Vec<Entry> = Vec::new();
    for (i, line) in text.split_terminator('\n').enumerate() {
        let refused = |why: String| PackError::Tsv { line: i + 1, why };
        let (fp, english) = line
            .split_once('\t')
            .ok_or_else(|| refused("no tab".into()))?;
        let fingerprint: Fingerprint = fp.parse().map_err(|e: PackError| refused(e.to_string()))?;
        if let Some(last) = entries.last() {
            if fingerprint == last.fingerprint {
                return Err(refused(format!("{fingerprint} again")));
            }
            if fingerprint < last.fingerprint {
                return Err(refused(format!("{fingerprint} out of order")));
            }
        }
        let english = unescape(english).map_err(|e| refused(e.to_string()))?;
        entries.push(Entry {
            fingerprint,
            english,
        });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint;

    #[test]
    fn escapes_round_trip_and_unknown_escapes_are_refused() {
        let text = "a\tb\nc\\d\re";
        assert_eq!(escape(text), r"a\tb\nc\\d\re");
        assert_eq!(unescape(&escape(text)).unwrap(), text);
        assert!(unescape(r"a\qb").is_err());
        assert!(unescape("trailing\\").is_err());
        // A raw tab or line break is never part of a field.
        assert!(unescape("a\tb").is_err());
        assert!(unescape("a\rb").is_err());
    }

    #[test]
    fn tsv_is_sorted_and_refuses_duplicates_and_disorder() {
        let mut entries = vec![
            Entry {
                fingerprint: fingerprint(b"two"),
                english: "Two\tlines\nhere".into(),
            },
            Entry {
                fingerprint: fingerprint(b"one"),
                english: "One".into(),
            },
        ];
        let text = write_tsv(&mut entries);
        let (a, b) = (fingerprint(b"one"), fingerprint(b"two"));
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(&format!("{lo}\t")), "{text}");
        assert!(lines[1].starts_with(&format!("{hi}\t")), "{text}");
        assert!(text.ends_with('\n'));
        let read = read_tsv(&text).unwrap();
        assert_eq!(read.len(), 2);
        assert!(read.iter().any(|e| e.english == "Two\tlines\nhere"));
        assert!(read_tsv("").unwrap().is_empty());
        for bad in [
            format!("{lo}\tA\n{lo}\tB\n"),
            format!("{hi}\tA\n{lo}\tB\n"),
            format!("{lo} A\n"),
            "zz\tA\n".to_string(),
        ] {
            assert!(read_tsv(&bad).is_err(), "{bad:?}");
        }
    }
}
