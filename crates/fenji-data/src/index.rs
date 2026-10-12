//! `_index`'s text: `<40 hex>\t<path>` lines (docs/re/m1-archives.md 3.2). A line starting with `-`
//! removes a path; a path ending in `$$` is a container. Uppercase hex is refused, as the engine's
//! keys are lowercase in every line seen (M2 plan Task 11, Step 6). A few paths aren't UTF-8
//! (m1-archives.md 3.4): those lines are counted, not mapped.
use std::collections::{BTreeMap, BTreeSet};

/// One line of `_index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line<'a> {
    Add { key: [u8; 20], path: &'a str },
    Remove { path: &'a str },
}

fn unhex(s: &str) -> Option<[u8; 20]> {
    if s.len() != 40
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// One line, without its newline; a trailing `\r` is ignored. `None` if it is neither form.
pub fn parse_line(line: &str) -> Option<Line<'_>> {
    let line = line.strip_suffix('\r').unwrap_or(line);
    if let Some(path) = line.strip_prefix('-') {
        return (!path.is_empty()).then_some(Line::Remove { path });
    }
    let (hex, path) = line.split_once('\t')?;
    if path.is_empty() {
        return None;
    }
    Some(Line::Add {
        key: unhex(hex)?,
        path,
    })
}

/// The paths `_index` leaves, each with its file's key.
#[derive(Debug, Default)]
pub struct PathMap {
    pub by_path: BTreeMap<String, [u8; 20]>,
    pub containers: BTreeSet<String>,
    /// Lines in neither form.
    pub bad_lines: usize,
    /// Lines that aren't UTF-8, so not parsed.
    pub non_utf8_lines: usize,
}

/// Applies `_index`'s lines in order. Blank lines are skipped.
pub fn build(raw: &[u8]) -> PathMap {
    let mut m = PathMap::default();
    for line in raw.split(|&b| b == b'\n') {
        let Ok(line) = std::str::from_utf8(line) else {
            m.non_utf8_lines += 1;
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(Line::Add { key, path }) => {
                if path.ends_with("$$") {
                    m.containers.insert(path.to_string());
                }
                m.by_path.insert(path.to_string(), key);
            }
            Some(Line::Remove { path }) => {
                m.by_path.remove(path);
                m.containers.remove(path);
            }
            None => m.bad_lines += 1,
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const K: &str = "00112233445566778899aabbccddeeff00112233";

    #[test]
    fn parses_adds_removes_and_containers() {
        let text = format!(
            "{K}\tdata/tables/item.tab\n{K}\tdata/maps/a.zip$$\n-data/old.tab\nnot a line\n\n{K}\tdata/old.tab\n"
        );
        let m = build(text.as_bytes());
        assert_eq!(m.by_path.len(), 3);
        assert!(m.by_path.contains_key("data/tables/item.tab"));
        assert!(m.containers.contains("data/maps/a.zip$$"));
        assert_eq!(m.bad_lines, 1);
    }

    #[test]
    fn a_later_remove_drops_the_path() {
        let text = format!("{K}\tdata/x.tab\n-data/x.tab\n");
        assert!(build(text.as_bytes()).by_path.is_empty());
    }

    #[test]
    fn key_must_be_40_lowercase_hex() {
        assert!(parse_line(&format!("{}\tp", &K[..39])).is_none());
        assert!(parse_line(&format!("{}\tp", K.to_uppercase())).is_none());
        assert!(parse_line(&format!("{K}\t")).is_none());
        assert!(matches!(
            parse_line(&format!("{K}\tp")),
            Some(Line::Add { path: "p", .. })
        ));
    }

    #[test]
    fn crlf_is_tolerated() {
        let m = build(format!("{K}\tdata/a.tab\r\n").as_bytes());
        assert!(m.by_path.contains_key("data/a.tab"));
    }

    #[test]
    fn a_line_that_is_not_utf8_is_counted_apart() {
        // _index names a few paths in bytes that aren't UTF-8 (docs/re/m1-archives.md 3.4).
        let mut raw = format!("{K}\tdata/a.tab\n{K}\tdata/").into_bytes();
        raw.extend_from_slice(b"\xb2\xe2.tab\n");
        let m = build(&raw);
        assert_eq!(m.by_path.len(), 1);
        assert_eq!((m.non_utf8_lines, m.bad_lines), (1, 0));
    }
}
