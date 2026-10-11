//! luatool's commands. Each returns a [`Summary`]; per-file failures are counted, never fatal.
//! Files are named by their engine path, `data/<dir>/<path>`.
use std::io::Write;

use anyhow::Result;
use fenji_data::ExeFs;

use crate::client::{Decoded, decode, selected};
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
