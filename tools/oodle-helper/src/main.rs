//! oodle-helper: the client's own Oodle DLL, for dev work. Windows target only; it runs under Wine
//! inside the sandbox, through scripts/spike/run-oodle-helper.sh.
//! - `oodle-helper DLL SPOOL` decodes every pending block in SPOOL/in into SPOOL/out.
//! - `oodle-helper DLL probe` says whether the DLL exports `OodleLZ_Compress`.
//! - `oodle-helper DLL compress IN OUT COMPRESSOR LEVEL` compresses each IN/*.raw, Fenji's own
//!   synthetic bytes, into OUT/<stem>.c<COMPRESSOR>.l<LEVEL>.oz, for fenji-oodle's test fixtures.
//!   An output is written only once the DLL decodes it back to its input.
//! - `oodle-helper DLL hash-parts CLIENT OUT_TSV` decodes every Oodle block of the client copy at
//!   CLIENT (`_index`'s, the containers' files' and the parts' live records') and writes one
//!   `key, block, raw length, SHA-256` line each to OUT_TSV, for `archive verify-oodle`.
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod dll;
mod spool;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fenji_data::archive::OodleSource;

const USAGE: &str = "usage: oodle-helper DLL SPOOL | oodle-helper DLL probe | oodle-helper DLL compress IN OUT COMPRESSOR LEVEL | oodle-helper DLL hash-parts CLIENT OUT_TSV";

/// What a command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Spool(PathBuf),
    Probe,
    Compress {
        input: PathBuf,
        output: PathBuf,
        compressor: i32,
        level: i32,
    },
    HashParts {
        client: PathBuf,
        out: PathBuf,
    },
}

/// The DLL's path and the mode `args` (the program's name first) ask for, or `None` for a usage
/// error. `probe`, `compress` and `hash-parts` name their modes, never a spool.
fn parse(args: &[String]) -> Option<(PathBuf, Mode)> {
    let [_, dll, rest @ ..] = args else {
        return None;
    };
    let mode = match rest {
        [m] if m == "probe" => Mode::Probe,
        [m, input, output, compressor, level] if m == "compress" => Mode::Compress {
            input: input.into(),
            output: output.into(),
            compressor: compressor.parse().ok()?,
            level: level.parse().ok()?,
        },
        [m, client, out] if m == "hash-parts" => Mode::HashParts {
            client: client.into(),
            out: out.into(),
        },
        [m, ..] if m == "probe" || m == "compress" || m == "hash-parts" => return None,
        [spool] => Mode::Spool(spool.into()),
        _ => return None,
    };
    Some((dll.into(), mode))
}

/// What `hash-parts` hashed: its blocks, the FAILED ones, and the files and blocks by where they
/// lie.
#[derive(Debug, Default)]
struct Totals {
    blocks: u64,
    failed: u64,
    walked: BTreeMap<OodleSource, (u64, u64)>,
}

impl std::fmt::Display for Totals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let walked = |s| self.walked.get(&s).copied().unwrap_or_default();
        let (index, inner, parts) = (
            walked(OodleSource::Index),
            walked(OodleSource::Container),
            walked(OodleSource::Part),
        );
        write!(
            f,
            "{} blocks hashed, {} FAILED: {} files and {} blocks in _index, {} files and {} blocks \
             in the containers, {} files and {} blocks in the parts",
            self.blocks, self.failed, index.0, index.1, inner.0, inner.1, parts.0, parts.1
        )
    }
}

/// Writes to `out` one `key\tblock\traw_len\tsha256` line for every Oodle block of the client copy
/// at `client`, as fenji-data's `each_oodle_blob` walks them, which `archive verify-oodle` walks
/// too: the SHA-256 of what `decode` makes of the block's stream, or FAILED. A block over 512 KiB,
/// or whose `u32` raw length disagrees with its blob header, is FAILED without a decode.
fn hash_parts_to(
    client: &Path,
    out: &mut impl Write,
    mut decode: impl FnMut(&[u8], usize) -> Option<Vec<u8>>,
) -> Result<Totals, fenji_data::Error> {
    let mut t = Totals::default();
    fenji_data::archive::each_oodle_blob(&client.join("data"), |source, name, blob| {
        let (_, blocks) = fenji_data::blocks(blob, 0)?;
        let walked = t.walked.entry(source).or_default();
        walked.0 += 1;
        walked.1 += blocks.len() as u64;
        for b in blocks {
            let raw = (b.raw_len <= fenji_data::MAX_OODLE_BLOCK)
                .then(|| fenji_data::oodle_payload(b.input, b.raw_len, b.index).ok())
                .flatten()
                .and_then(|stream| decode(stream, b.raw_len))
                .filter(|raw| raw.len() == b.raw_len);
            let hash = raw.map_or_else(|| "FAILED".into(), |raw| fenji_data::sha256_hex(&raw));
            t.blocks += 1;
            t.failed += u64::from(hash == "FAILED");
            writeln!(out, "{name}\t{}\t{}\t{hash}", b.index, b.raw_len)?;
        }
        Ok::<(), fenji_data::Error>(())
    })?;
    Ok(t)
}

/// The stems of the files in `dir` named `<stem>.raw`, sorted. Anything else is ignored.
fn raw_inputs(dir: &Path) -> io::Result<Vec<String>> {
    let mut stems = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if let Some(stem) = name.strip_suffix(".raw")
            && !stem.is_empty()
            && entry.file_type()?.is_file()
        {
            stems.push(stem.to_string());
        }
    }
    stems.sort();
    Ok(stems)
}

/// A fixture's file name: `<stem>.c<compressor>.l<level>.oz`.
fn fixture_name(stem: &str, compressor: i32, level: i32) -> String {
    format!("{stem}.c{compressor}.l{level}.oz")
}

#[cfg(windows)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some((dll_path, mode)) = parse(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let oodle = match dll::Oodle::load(&dll_path) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    match mode {
        Mode::Spool(spool_dir) => decode_spool(&oodle, &spool_dir),
        Mode::Probe => probe(&oodle),
        Mode::Compress {
            input,
            output,
            compressor,
            level,
        } => compress(&oodle, &input, &output, compressor, level),
        Mode::HashParts { client, out } => hash_parts(&oodle, &client, &out),
    }
}

/// Writes OUT_TSV through a temporary file beside it, and prints the totals. Exit 0 once every
/// block is hashed, FAILED lines included: those are data, not a failure here. Exit 1 if the walk
/// can't be done.
#[cfg(windows)]
fn hash_parts(oodle: &dll::Oodle, client: &Path, out: &Path) -> ExitCode {
    match write_hashes(oodle, client, out) {
        Ok(totals) => {
            println!("{totals}; wrote {}", out.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("hash-parts: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(windows)]
fn write_hashes(
    oodle: &dll::Oodle,
    client: &Path,
    out: &Path,
) -> Result<Totals, fenji_data::Error> {
    let (Some(dir), Some(name)) = (out.parent(), out.file_name()) else {
        let what = format!("{} isn't a file's path", out.display());
        return Err(fenji_data::Error::OutDir(what));
    };
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    let mut w = io::BufWriter::new(std::fs::File::create(&tmp)?);
    let totals = hash_parts_to(client, &mut w, |stream, raw_len| {
        oodle.decompress(stream, raw_len)
    })?;
    w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    std::fs::rename(&tmp, out)?;
    Ok(totals)
}

/// Decodes every pending block in SPOOL/in into SPOOL/out.
#[cfg(windows)]
fn decode_spool(oodle: &dll::Oodle, spool_dir: &Path) -> ExitCode {
    let pending = match spool::pending(spool_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}: {e}", spool_dir.display());
            return ExitCode::from(1);
        }
    };
    for name in &pending.refused {
        eprintln!(
            "{name}: not a spool name, or more than {} bytes: not decoded",
            spool::MAX_RAW
        );
    }
    let jobs = &pending.jobs;
    let (mut done, mut failed) = (0usize, pending.refused.len());
    for (name, raw_len) in jobs {
        let input = match std::fs::read(spool_dir.join("in").join(name)) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{name}: {e}");
                failed += 1;
                continue;
            }
        };
        let Some(stream) = spool::oodle_stream(&input, *raw_len) else {
            eprintln!("{name}: the block doesn't start with its raw length");
            failed += 1;
            continue;
        };
        match oodle.decompress(stream, *raw_len) {
            Some(raw) => match spool::write_atomic(&spool_dir.join("out"), name, &raw) {
                Ok(()) => done += 1,
                Err(e) => {
                    eprintln!("{name}: {e}");
                    failed += 1;
                }
            },
            None => {
                eprintln!("{name}: the DLL did not return {raw_len} bytes");
                failed += 1;
            }
        }
    }
    println!(
        "{done} decoded, {failed} failed, {} pending before",
        jobs.len()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// Says whether the DLL exports `OodleLZ_Compress`: exit 0 if it does, 1 if not.
#[cfg(windows)]
fn probe(oodle: &dll::Oodle) -> ExitCode {
    if oodle.can_compress() {
        println!("OodleLZ_Compress resolves");
        ExitCode::SUCCESS
    } else {
        println!("OodleLZ_Compress doesn't resolve");
        ExitCode::from(1)
    }
}

/// Compresses each `input/<stem>.raw` into `output`, as [`fixture_name`] names it. Exit 1 if any
/// input fails, or if there are none.
#[cfg(windows)]
fn compress(
    oodle: &dll::Oodle,
    input: &Path,
    output: &Path,
    compressor: i32,
    level: i32,
) -> ExitCode {
    let stems = match raw_inputs(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}: {e}", input.display());
            return ExitCode::from(1);
        }
    };
    if stems.is_empty() {
        eprintln!("{}: no .raw inputs", input.display());
        return ExitCode::from(1);
    }
    let (mut done, mut failed) = (0usize, 0usize);
    for stem in &stems {
        let raw = match std::fs::read(input.join(format!("{stem}.raw"))) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{stem}.raw: {e}");
                failed += 1;
                continue;
            }
        };
        let Some(stream) = oodle.compress(compressor, level, &raw) else {
            eprintln!("{stem}.raw: the DLL did not compress it");
            failed += 1;
            continue;
        };
        // A fixture is only worth committing if the DLL's own decoder gets the input back.
        if oodle.decompress(&stream, raw.len()).as_deref() != Some(&raw[..]) {
            eprintln!("{stem}.raw: the DLL did not decode its output back to the input");
            failed += 1;
            continue;
        }
        let name = fixture_name(stem, compressor, level);
        match spool::write_atomic(output, &name, &stream) {
            Ok(()) => {
                println!("{name}\t{}\t{}", raw.len(), stream.len());
                done += 1;
            }
            Err(e) => {
                eprintln!("{name}: {e}");
                failed += 1;
            }
        }
    }
    println!("{done} compressed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!(
        "oodle-helper is a Windows program: build it with --target x86_64-pc-windows-gnu and run it through scripts/spike/run-oodle-helper.sh"
    );
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_command_lines() {
        assert_eq!(
            parse(&args(&["h", "D", "S"])),
            Some(("D".into(), Mode::Spool("S".into()))),
            "the spool mode keeps its two-argument form"
        );
        assert_eq!(
            parse(&args(&["h", "D", "probe"])),
            Some(("D".into(), Mode::Probe))
        );
        assert_eq!(
            parse(&args(&["h", "D", "compress", "I", "O", "8", "-4"])),
            Some((
                "D".into(),
                Mode::Compress {
                    input: "I".into(),
                    output: "O".into(),
                    compressor: 8,
                    level: -4
                }
            ))
        );
        assert_eq!(
            parse(&args(&["h", "D", "hash-parts", "C", "T"])),
            Some((
                "D".into(),
                Mode::HashParts {
                    client: "C".into(),
                    out: "T".into()
                }
            ))
        );
        for bad in [
            &["h"][..],
            &["h", "D"],
            &["h", "D", "S", "x"],
            &["h", "D", "probe", "x"],
            &["h", "D", "compress"],
            &["h", "D", "compress", "I", "O", "8"],
            &["h", "D", "compress", "I", "O", "8", "4", "x"],
            &["h", "D", "compress", "I", "O", "kraken", "4"],
            &["h", "D", "compress", "I", "O", "8", "4.5"],
            &["h", "D", "hash-parts"],
            &["h", "D", "hash-parts", "C"],
            &["h", "D", "hash-parts", "C", "T", "x"],
        ] {
            assert_eq!(parse(&args(bad)), None, "{bad:?}");
        }
    }

    /// The DLL's stand-in: fenji-data's own decoder, on the stream behind its raw length.
    fn native(stream: &[u8], raw_len: usize) -> Option<Vec<u8>> {
        use fenji_data::{BuiltinCodecs, Codec, Decompressor};
        let block = fenji_data::test_util::oodle_block(raw_len, stream);
        BuiltinCodecs
            .decompress_block(Codec::Oodle, &block, raw_len, 0)
            .ok()
    }

    /// A client copy's `data/`: `_index` in two memset blocks, and a part with a memset file, a
    /// container of one Oodle file and a stored one, and a block whose raw length disagrees.
    fn client() -> tempfile::TempDir {
        use fenji_data::test_util::{container, oodle_memset_blob, stored_blob, zfs_part};
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(
            data.join("_index"),
            oodle_memset_blob(600_000, 512 * 1024, 1),
        )
        .unwrap();
        let inner = container(&[
            ([0xD1; 20], "a.dds", oodle_memset_blob(10, 8, 3)),
            ([0xD2; 20], "b.txt", stored_blob(b"plain")),
        ]);
        let mut bad_prefix = oodle_memset_blob(100, 512 * 1024, 4);
        bad_prefix[36] = 99; // the block's u32 raw length, 100 in the blob header
        let part = zfs_part(&[vec![
            (1, [0xA; 20], oodle_memset_blob(1000, 512 * 1024, 2)),
            (1, [0xC; 20], stored_blob(&inner)),
            (1, [0xB; 20], bad_prefix),
        ]]);
        std::fs::write(data.join("data000"), part).unwrap();
        dir
    }

    #[test]
    fn hash_parts_writes_a_line_per_oodle_block() {
        let client = client();
        let mut tsv = Vec::new();
        let totals = hash_parts_to(client.path(), &mut tsv, native).unwrap();
        let hash = |byte: u8, n: usize| fenji_data::sha256_hex(&vec![byte; n]);
        let inner = format!("{}/0/{}", "0c".repeat(20), "d1".repeat(20));
        let want = [
            format!("_index\t0\t524288\t{}", hash(1, 524_288)),
            format!("_index\t1\t75712\t{}", hash(1, 75_712)),
            format!("{inner}\t0\t8\t{}", hash(3, 8)),
            format!("{inner}\t1\t2\t{}", hash(3, 2)),
            format!("{}\t0\t1000\t{}", "0a".repeat(20), hash(2, 1000)),
            format!("{}\t0\t100\tFAILED", "0b".repeat(20)),
        ];
        assert_eq!(String::from_utf8(tsv).unwrap(), want.join("\n") + "\n");
        assert_eq!(
            totals.to_string(),
            "6 blocks hashed, 1 FAILED: 1 files and 2 blocks in _index, \
             1 files and 2 blocks in the containers, 2 files and 2 blocks in the parts"
        );
    }

    #[test]
    fn a_block_the_dll_refuses_is_a_failed_line_not_a_failure() {
        let client = client();
        let mut tsv = Vec::new();
        let mut asked = Vec::new();
        let totals = hash_parts_to(client.path(), &mut tsv, |stream, raw_len| {
            asked.push(raw_len);
            (raw_len != 1000).then(|| native(stream, raw_len)).flatten()
        })
        .unwrap();
        let tsv = String::from_utf8(tsv).unwrap();
        assert!(
            tsv.contains(&format!("{}\t0\t1000\tFAILED\n", "0a".repeat(20))),
            "{tsv}"
        );
        assert_eq!(totals.failed, 2);
        // The block whose prefix disagrees never reaches the DLL.
        assert_eq!(asked, [524_288, 75_712, 8, 2, 1000]);
    }

    #[test]
    fn hash_parts_bounds_a_block_before_the_dll_sees_it() {
        // Review Focus 5 on the DLL's side: a block over 512 KiB is FAILED, never decoded.
        use fenji_data::test_util::{oodle_memset_blob, zfs_part};
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::write(data.join("_index"), oodle_memset_blob(10, 512 * 1024, 1)).unwrap();
        let big = oodle_memset_blob(600 * 1024, 600 * 1024, 7);
        std::fs::write(data.join("data000"), zfs_part(&[vec![(1, [0xA; 20], big)]])).unwrap();
        let mut tsv = Vec::new();
        let mut asked = Vec::new();
        hash_parts_to(dir.path(), &mut tsv, |stream, raw_len| {
            asked.push(raw_len);
            native(stream, raw_len)
        })
        .unwrap();
        assert!(
            String::from_utf8(tsv)
                .unwrap()
                .ends_with(&format!("{}\t0\t614400\tFAILED\n", "0a".repeat(20)))
        );
        assert_eq!(asked, [10]);
    }

    #[test]
    fn the_inputs_are_the_raw_files_by_stem() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["b-2-2.raw", "a-1-1.raw", "notes.txt", ".raw", "c.raw.tmp"] {
            std::fs::write(tmp.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(tmp.path().join("d.raw")).unwrap();
        assert_eq!(raw_inputs(tmp.path()).unwrap(), ["a-1-1", "b-2-2"]);
        assert_eq!(fixture_name("a-1-1", 8, 4), "a-1-1.c8.l4.oz");
    }
}
