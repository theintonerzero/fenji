//! oodle-helper: the client's own Oodle DLL, for dev work. Windows target only; it runs under Wine
//! inside the sandbox, through scripts/spike/run-oodle-helper.sh.
//! - `oodle-helper DLL SPOOL` decodes every pending block in SPOOL/in into SPOOL/out.
//! - `oodle-helper DLL probe` says whether the DLL exports `OodleLZ_Compress`.
//! - `oodle-helper DLL compress IN OUT COMPRESSOR LEVEL` compresses each IN/*.raw, Fenji's own
//!   synthetic bytes, into OUT/<stem>.c<COMPRESSOR>.l<LEVEL>.oz, for fenji-oodle's test fixtures.
//!   An output is written only once the DLL decodes it back to its input.
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod dll;
mod spool;

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: oodle-helper DLL SPOOL | oodle-helper DLL probe | oodle-helper DLL compress IN OUT COMPRESSOR LEVEL";

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
}

/// The DLL's path and the mode `args` (the program's name first) ask for, or `None` for a usage
/// error. `probe` and `compress` name their modes, never a spool.
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
        [m, ..] if m == "probe" || m == "compress" => return None,
        [spool] => Mode::Spool(spool.into()),
        _ => return None,
    };
    Some((dll.into(), mode))
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
    }
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
        ] {
            assert_eq!(parse(&args(bad)), None, "{bad:?}");
        }
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
