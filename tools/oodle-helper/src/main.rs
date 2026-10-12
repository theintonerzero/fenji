//! oodle-helper DLL SPOOL: decodes every pending block in SPOOL/in into SPOOL/out with the client's
//! own Oodle DLL. Windows target only; runs under Wine through scripts/spike/run-oodle-helper.sh.
#![cfg_attr(not(windows), allow(dead_code))]
#[cfg(windows)]
mod dll;
mod spool;

use std::process::ExitCode;

#[cfg(windows)]
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, dll_path, spool_dir] = args.as_slice() else {
        eprintln!("usage: oodle-helper DLL SPOOL");
        return ExitCode::from(2);
    };
    let spool_dir = std::path::Path::new(spool_dir);
    let oodle = match dll::Oodle::load(std::path::Path::new(dll_path)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(1);
        }
    };
    let jobs = match spool::pending(spool_dir) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("{}: {e}", spool_dir.display());
            return ExitCode::from(1);
        }
    };
    let (mut done, mut failed) = (0usize, 0usize);
    for (name, raw_len) in &jobs {
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

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!(
        "oodle-helper is a Windows program: build it with --target x86_64-pc-windows-gnu and run it through scripts/spike/run-oodle-helper.sh"
    );
    ExitCode::from(2)
}
