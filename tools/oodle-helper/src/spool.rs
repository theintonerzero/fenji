//! The spool's file names and atomic writes; the same layout as fenji-data's OodleSpool.
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The most a block may decode to (the archives' blocks are 512 KiB).
pub const MAX_RAW: usize = 64 << 20;

/// `<40 hex>-<raw length>.bin` → the raw length.
pub fn parse_name(name: &str) -> Option<usize> {
    let stem = name.strip_suffix(".bin")?;
    let (hex, len) = stem.split_once('-')?;
    if hex.len() != 40
        || !hex
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return None;
    }
    if len.is_empty() || !len.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    len.parse().ok().filter(|&n| n <= MAX_RAW)
}

/// The Oodle stream inside a block: an archive block starts with its raw length as a `u32`, and
/// the engine's Oodle decompressor (FUN_14024E060) passes `OodleLZ_Decompress` the bytes after it.
/// `None` if the block is shorter than that or its first `u32` isn't `raw_len`.
pub fn oodle_stream(block: &[u8], raw_len: usize) -> Option<&[u8]> {
    let (head, stream) = block.split_first_chunk::<4>()?;
    (u32::from_le_bytes(*head) as usize == raw_len).then_some(stream)
}

/// What `pending` found in `spool/in`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Pending {
    /// Inputs without an output in `spool/out`, with their raw lengths, sorted by name.
    pub jobs: Vec<(String, usize)>,
    /// `.bin` names that aren't spool names, or ask for more than `MAX_RAW`: reported, not decoded.
    pub refused: Vec<String>,
}

/// The inputs in `spool/in` that wait for an output. Other files (temporary files, say) are ignored.
pub fn pending(spool: &Path) -> io::Result<Pending> {
    let mut p = Pending::default();
    for entry in fs::read_dir(spool.join("in"))? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        match parse_name(&name) {
            Some(raw) if !spool.join("out").join(&name).exists() => p.jobs.push((name, raw)),
            Some(_) => {}
            None if name.ends_with(".bin") => p.refused.push(name),
            None => {}
        }
    }
    p.jobs.sort();
    p.refused.sort();
    Ok(p)
}

/// Writes `dir/name` through a temporary file, so a reader never sees a partial output.
pub fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

    #[test]
    fn names() {
        assert_eq!(parse_name(&format!("{H}-524288.bin")), Some(524_288));
        assert_eq!(parse_name(&format!("{H}-0.bin")), Some(0));
        for bad in [
            format!("{H}-1.tmp"),
            format!("{H}.bin"),
            format!("{}-1.bin", H.to_uppercase()),
            format!("{H}-x.bin"),
            format!("{H}--1.bin"),
            format!("{H}-{}.bin", MAX_RAW + 1),
            "../x-1.bin".to_string(),
        ] {
            assert_eq!(parse_name(&bad), None, "{bad}");
        }
    }

    #[test]
    fn a_block_starts_with_its_raw_length() {
        let block = [0x00, 0x00, 0x08, 0x00, 0x8c, 0x06, 0xaa];
        assert_eq!(oodle_stream(&block, 0x8_0000), Some(&block[4..]));
        assert_eq!(oodle_stream(&block, 0x8_0001), None, "another raw length");
        assert_eq!(oodle_stream(&block[..3], 0x8_0000), None, "no prefix");
    }

    #[test]
    fn pending_skips_done_and_foreign_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("in")).unwrap();
        fs::create_dir_all(tmp.path().join("out")).unwrap();
        let a = format!("{H}-1.bin");
        let b = format!("{}-2.bin", "b".repeat(40));
        fs::write(tmp.path().join("in").join(&a), b"x").unwrap();
        fs::write(tmp.path().join("in").join(&b), b"y").unwrap();
        fs::write(tmp.path().join("in").join("notes.txt"), b"z").unwrap();
        let too_big = format!("{H}-{}.bin", MAX_RAW + 1);
        fs::write(tmp.path().join("in").join(&too_big), b"w").unwrap();
        write_atomic(&tmp.path().join("out"), &a, b"1").unwrap();
        let p = pending(tmp.path()).unwrap();
        assert_eq!(p.jobs, [(b, 2)]);
        assert_eq!(
            p.refused,
            [too_big],
            "a .bin name it can't take is reported, not skipped"
        );
        assert!(!tmp.path().join("out").join(format!(".{a}.tmp")).exists());
    }
}
