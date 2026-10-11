//! luatool's commands. Each returns a [`Summary`]; per-file failures are counted, never fatal.
//! Files are named by their engine path, `data/<dir>/<path>`.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use fenji_data::ExeFs;
use fenji_lua::Dialect;

use crate::client::{Decoded, area, decode, selected};
use crate::decompile::{Outcome, Unluac};
use crate::output::Out;

/// Counts for one command: printed at the end, with the failures written to
/// `<command>-failures.tsv`.
#[derive(Debug, Default)]
pub struct Summary {
    pub lua: u64,
    pub other: u64,
    pub undecodable: u64,
    pub bad_chunks: u64,
    pub failures: Vec<(String, String)>,
}

impl Summary {
    pub fn fail(&mut self, path: &str, why: impl std::fmt::Display) {
        self.failures.push((path.to_string(), why.to_string()));
    }

    /// One line for humans.
    pub fn line(&self, command: &str) -> String {
        format!(
            "{command}: {} Lua chunks, {} other files, {} undecodable entries, {} chunks refused or unparsable, {} failures listed",
            self.lua,
            self.other,
            self.undecodable,
            self.bad_chunks,
            self.failures.len()
        )
    }

    /// Writes the failures into the output directory.
    pub fn write_failures(&self, out: &Out, command: &str) -> Result<()> {
        let mut text = String::from("path\treason\n");
        for (path, why) in &self.failures {
            text.push_str(&format!("{path}\t{}\n", why.replace(['\t', '\n'], " ")));
        }
        std::fs::write(out.root().join(format!("{command}-failures.tsv")), text)?;
        Ok(())
    }
}

/// `list`: one TSV line per selected file: path, stored size, raw size, kind, status.
pub fn list(fs: &ExeFs, prefix: &str, w: &mut impl Write) -> Result<Summary> {
    let mut s = Summary::default();
    writeln!(w, "path\tstored\traw\tkind\tstatus")?;
    for e in selected(fs, prefix) {
        let path = e.engine_path();
        let raw = fs
            .header(e)
            .map_or_else(|_| "?".to_string(), |h| h.raw_size.to_string());
        let (kind, status) = match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match fenji_lua::parse(&bytes, dialect) {
                    Ok(_) => ("lua", "ok".to_string()),
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&path, &err);
                        ("lua", format!("bad chunk: {err}"))
                    }
                }
            }
            Decoded::Other(_) => {
                s.other += 1;
                ("file", "ok".to_string())
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&path, &why);
                ("?", format!("undecodable: {why}"))
            }
        };
        writeln!(w, "{path}\t{}\t{raw}\t{kind}\t{status}", e.stored_size)?;
    }
    Ok(s)
}

/// `extract`: every decodable file's content, exactly as the client stores it, under `out`.
pub fn extract(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        let path = e.engine_path();
        let bytes = match decode(fs, e) {
            Decoded::Lua(bytes, _) => {
                s.lua += 1;
                bytes
            }
            Decoded::Other(bytes) => {
                s.other += 1;
                bytes
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&path, why);
                continue;
            }
        };
        if out.write(&path, "", &bytes)?.is_none() {
            s.fail(&path, "unsafe path in the registry; not written");
        }
    }
    Ok(s)
}

/// `disasm`: a listing, `<path>.dis`, of every Lua chunk that parses.
pub fn disasm(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        let path = e.engine_path();
        match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match fenji_lua::parse(&bytes, dialect) {
                    Ok(chunk) => {
                        if out
                            .write(&path, ".dis", fenji_lua::disassemble(&chunk).as_bytes())?
                            .is_none()
                        {
                            s.fail(&path, "unsafe path in the registry; not written");
                        }
                    }
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&path, err);
                    }
                }
            }
            Decoded::Other(_) => s.other += 1,
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&path, why);
            }
        }
    }
    Ok(s)
}

/// A Lua chunk normalised to stock 5.3 bytes.
fn stock_bytes(bytes: &[u8], dialect: Dialect) -> fenji_lua::Result<Vec<u8>> {
    let chunk = fenji_lua::parse(bytes, dialect)?;
    let (normal, _) = fenji_lua::normalize(&chunk)?;
    Ok(fenji_lua::write(&normal, Dialect::Stock53))
}

/// `normalize`: each Lua chunk rewritten to stock Lua 5.3 (still `<path>`, a .luac) under `out`.
pub fn normalize(fs: &ExeFs, prefix: &str, out: &mut Out) -> Result<Summary> {
    let mut s = Summary::default();
    for e in selected(fs, prefix) {
        let path = e.engine_path();
        match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => {
                s.lua += 1;
                match stock_bytes(&bytes, dialect) {
                    Ok(stock) => {
                        if out.write(&path, "", &stock)?.is_none() {
                            s.fail(&path, "unsafe path in the registry; not written");
                        }
                    }
                    Err(err) => {
                        s.bad_chunks += 1;
                        s.fail(&path, err);
                    }
                }
            }
            Decoded::Other(_) => s.other += 1,
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&path, why);
            }
        }
    }
    Ok(s)
}

/// Per-area decompile counts: chunks, decompiled, refused by the normaliser, unluac failed,
/// timed out.
type AreaCounts = [u64; 5];

/// `decompile`: normalise each chunk into `out/.stock/`, decompile it with unluac into
/// `out/<path without .luac>.lua`, and write per-area counts to `out/decompile-summary.tsv`.
pub fn decompile(
    fs: &ExeFs,
    prefix: &str,
    out: &mut Out,
    unluac: &Unluac,
    workers: usize,
) -> Result<Summary> {
    let mut s = Summary::default();
    let mut areas: BTreeMap<String, AreaCounts> = BTreeMap::new();
    let mut jobs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut job_info: Vec<(String, String)> = Vec::new(); // (area, path) per job
    for e in selected(fs, prefix) {
        let path = e.engine_path();
        let (bytes, dialect) = match decode(fs, e) {
            Decoded::Lua(bytes, dialect) => (bytes, dialect),
            Decoded::Other(_) => {
                s.other += 1;
                continue;
            }
            Decoded::Failed(why) => {
                s.undecodable += 1;
                s.fail(&path, why);
                continue;
            }
        };
        s.lua += 1;
        let a = area(&path);
        areas.entry(a.clone()).or_default()[0] += 1;
        let stock = match stock_bytes(&bytes, dialect) {
            Ok(stock) => stock,
            Err(err) => {
                areas.entry(a).or_default()[2] += 1;
                s.bad_chunks += 1;
                s.fail(&path, err);
                continue;
            }
        };
        let name = path
            .strip_suffix(".luac")
            .map_or_else(|| format!("{path}.lua"), |p| format!("{p}.lua"));
        let input = out.write(&format!(".stock/{path}"), "", &stock)?;
        let output = out.target(&name, "")?;
        let (Some(input), Some(output)) = (input, output) else {
            s.fail(&path, "unsafe path in the registry; not written");
            continue;
        };
        jobs.push((input, output));
        job_info.push((a, path));
    }
    for ((a, path), outcome) in job_info.iter().zip(unluac.run_all(&jobs, workers)) {
        let counts = areas.entry(a.clone()).or_default();
        match outcome {
            Outcome::Ok => counts[1] += 1,
            Outcome::Failed => {
                counts[3] += 1;
                s.fail(path, "unluac failed (see the .lua.err next to the output)");
            }
            Outcome::TimedOut => {
                counts[4] += 1;
                s.fail(path, "unluac timed out");
            }
        }
    }
    let mut tsv =
        String::from("area\tchunks\tdecompiled\tnormaliser_refused\tunluac_failed\ttimed_out\n");
    for (a, [n, ok, refused, failed, timeout]) in &areas {
        tsv.push_str(&format!("{a}\t{n}\t{ok}\t{refused}\t{failed}\t{timeout}\n"));
    }
    std::fs::write(out.root().join("decompile-summary.tsv"), tsv)?;
    Ok(s)
}
