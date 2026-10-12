//! archive: the SoLO data archives' structure (M1 spec §5, docs/re/m1-archives.md). Oodle (codec 4)
//! is read only through the spool that scripts/spike/run-oodle-helper.sh decodes (M2 plan Task 11):
//! `paths` decodes `_index` that way; `extract` still writes stored files only. Writes only under
//! re/.
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fenji_data::archive::{
    CONTAINER_MAGIC, Record, STATUS_USED, read_blob_header, read_part_header,
};
use fenji_data::oodle_spool::OodleSpool;
use fenji_data::{Codec, FIXED_LEN, index};
use sha1::{Digest, Sha1};

/// The SoLO data archives, read without decompression. Writes only under re/.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// The client copy (default: $FENJI_CLIENT_PATH)
    #[arg(long, global = true)]
    client: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Per part: index blocks, records by state, live records by codec, containers, and live
    /// records that disagree with their blob
    Info,
    /// One part's records as TSV: slot, key, offset, size, status, codec, raw size, stored size
    List {
        #[arg(long)]
        part: u32,
    },
    /// Write one part's stored (uncompressed) files under OUT/data0NN (inside re/), named by key
    Extract {
        #[arg(long)]
        part: u32,
        #[arg(long)]
        stored_only: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// The _index blob header
    Index,
    /// Decode _index through the Oodle spool and write OUT/paths.tsv
    ///
    /// Columns: key, path, part (3 digits), offset (hex), size, codec (stored, lz4, oodle, unknown,
    /// or bad-header when the blob header can't be read); `-` in the last four where no live record
    /// holds the key. OUT and the spool must be inside re/. Exit 3: blocks wait for
    /// scripts/spike/run-oodle-helper.sh (not a gate event).
    Paths {
        #[arg(long, default_value = "re/oodle-spool")]
        spool: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn part_path(client: &Path, n: u32) -> PathBuf {
    client.join("data").join(format!("data{n:03}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A record's state as the engine sees it (FUN_1400bff10).
fn status(r: &Record) -> &'static str {
    match (r.status == STATUS_USED, r.crc_ok()) {
        (true, true) => "used",
        (true, false) => "crc-bad",
        (false, _) => "deleted",
    }
}

/// The first four bytes of the stored blob at `offset`, if its content has four.
fn content_magic(f: &mut File, offset: u64, raw_size: u64) -> Result<Option<[u8; 4]>> {
    if raw_size < 4 {
        return Ok(None);
    }
    let mut m = [0; 4];
    f.seek(SeekFrom::Start(offset + FIXED_LEN as u64))?;
    f.read_exact(&mut m)?;
    Ok(Some(m))
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let client = match cli.client {
        Some(c) => c,
        None => PathBuf::from(
            std::env::var_os("FENJI_CLIENT_PATH")
                .context("pass --client or set FENJI_CLIENT_PATH")?,
        ),
    };
    match cli.command {
        Command::Info => info(&client),
        Command::List { part } => list(&client, part),
        Command::Paths { spool, out } => return paths(&client, &spool, &out),
        Command::Extract {
            part,
            stored_only,
            out,
        } => {
            if !stored_only {
                bail!(
                    "only --stored-only is available: Oodle (codec 4) waits on decision gate #39"
                );
            }
            extract_stored(&client, part, &out)
        }
        Command::Index => {
            let h = read_blob_header(&mut File::open(client.join("data").join("_index"))?, 0)?;
            println!(
                "raw {} stored {} timestamp {} block size {} codec {} ({}) check {:#06x} blocks {}",
                h.raw_size,
                h.stored_size,
                h.timestamp,
                h.block_size,
                h.codec.as_u16(),
                h.codec.name(),
                h.check,
                h.block_sizes.len()
            );
            Ok(())
        }
    }?;
    Ok(ExitCode::SUCCESS)
}

/// Where a live record keeps a file: part, offset, stored size, codec.
struct Place {
    part: u32,
    offset: u32,
    size: u32,
    codec: &'static str,
}

/// Decodes `_index` through the spool, then writes OUT/paths.tsv. Exits 3 when blocks wait for the
/// helper.
fn paths(client: &Path, spool: &Path, out: &Path) -> Result<ExitCode> {
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let out = fenji_data::re_dir::resolve_out_dir(&repo, out)?;
    // The spool holds the client's compressed blocks, so it stays under re/ too.
    let spool = OodleSpool::new(&fenji_data::re_dir::resolve_out_dir(&repo, spool)?)?;
    let index_path = client.join("data").join("_index");
    let bytes = std::fs::read(&index_path).with_context(|| index_path.display().to_string())?;
    let queued = spool.spool_blob(&bytes, 0)?;
    if queued > 0 {
        println!("{queued} blocks spooled: run scripts/spike/run-oodle-helper.sh, then re-run");
        return Ok(ExitCode::from(3));
    }
    let raw = match fenji_data::decode_blob(&bytes, 0, &spool) {
        Ok(raw) => raw,
        Err(fenji_data::Error::OodlePending(name)) => {
            println!(
                "{name} and maybe more wait in the spool: run scripts/spike/run-oodle-helper.sh, then re-run"
            );
            return Ok(ExitCode::from(3));
        }
        Err(e) => return Err(e.into()),
    };
    let map = index::build(&raw);

    let (mut places, mut live, mut repeated, mut unreadable) =
        (HashMap::new(), 0usize, 0usize, 0usize);
    for n in 0..1000 {
        let path = part_path(client, n);
        if !path.exists() {
            break;
        }
        let h = read_part_header(&path).with_context(|| path.display().to_string())?;
        let mut f = File::open(&path)?;
        for r in h.live() {
            live += 1;
            if places.contains_key(&r.key) {
                repeated += 1;
                continue;
            }
            let codec = match read_blob_header(&mut f, u64::from(r.offset)) {
                Ok(b) => b.codec.name(),
                Err(_) => {
                    unreadable += 1;
                    "bad-header"
                }
            };
            places.insert(
                r.key,
                Place {
                    part: n,
                    offset: r.offset,
                    size: r.size,
                    codec,
                },
            );
        }
    }

    let tsv = out.join("paths.tsv");
    let mut w = BufWriter::new(create_replacing(&tsv)?);
    writeln!(w, "key\tpath\tpart\toffset\tsize\tcodec")?;
    let mut found = 0usize;
    for (path, key) in &map.by_path {
        match places.get(key) {
            Some(p) => {
                found += 1;
                writeln!(
                    w,
                    "{}\t{path}\t{:03}\t{:#x}\t{}\t{}",
                    hex(key),
                    p.part,
                    p.offset,
                    p.size,
                    p.codec
                )?;
            }
            None => writeln!(w, "{}\t{path}\t-\t-\t-\t-", hex(key))?,
        }
    }
    w.flush()?;
    println!(
        "{} paths ({} containers); {} bad lines, {} lines not UTF-8, {} paths named again; {found} found in a part, {} not found; {live} live records, {repeated} repeating a key, {unreadable} with an unreadable blob header; wrote {}",
        map.by_path.len(),
        map.containers.len(),
        map.bad_lines,
        map.non_utf8_lines,
        map.replaced,
        map.by_path.len() - found,
        tsv.display()
    );
    Ok(ExitCode::SUCCESS)
}

fn info(client: &Path) -> Result<()> {
    println!(
        "part\tblocks\tlive\tdeleted\tcrc_bad\tstored\tlz4\toodle\tother\tcontainers\tsize_mismatch\tbad_header"
    );
    for n in 0..1000 {
        let path = part_path(client, n);
        if !path.exists() {
            break;
        }
        let h = read_part_header(&path).with_context(|| path.display().to_string())?;
        let mut f = File::open(&path)?;
        let mut c: BTreeMap<&str, u64> = BTreeMap::new();
        for r in &h.records {
            *c.entry(status(r)).or_default() += 1;
        }
        for r in h.live() {
            let offset = u64::from(r.offset);
            let Ok(b) = read_blob_header(&mut f, offset) else {
                *c.entry("bad_header").or_default() += 1;
                continue;
            };
            *c.entry(b.codec.name()).or_default() += 1;
            if b.stored_size != u64::from(r.size) {
                *c.entry("size_mismatch").or_default() += 1;
            }
            if b.codec == Codec::Stored
                && content_magic(&mut f, offset, b.raw_size)? == Some(*CONTAINER_MAGIC)
            {
                *c.entry("containers").or_default() += 1;
            }
        }
        let get = |k: &str| c.get(k).copied().unwrap_or(0);
        println!(
            "{n:03}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            h.blocks.len(),
            get("used"),
            get("deleted"),
            get("crc-bad"),
            get("stored"),
            get("lz4"),
            get("oodle"),
            get("unknown"),
            get("containers"),
            get("size_mismatch"),
            get("bad_header")
        );
    }
    Ok(())
}

fn list(client: &Path, part: u32) -> Result<()> {
    let path = part_path(client, part);
    let h = read_part_header(&path).with_context(|| path.display().to_string())?;
    let mut f = File::open(&path)?;
    println!("slot\tkey\toffset\tsize\tstatus\tcodec\traw\tstored");
    for r in &h.records {
        let (codec, raw, stored) = match read_blob_header(&mut f, u64::from(r.offset)) {
            Ok(b) => (
                b.codec.name().to_string(),
                b.raw_size.to_string(),
                b.stored_size.to_string(),
            ),
            Err(e) => (format!("bad header: {e}"), "?".into(), "?".into()),
        };
        println!(
            "{}\t{}\t{:#x}\t{}\t{}\t{codec}\t{raw}\t{stored}",
            r.slot,
            hex(&r.key),
            r.offset,
            r.size,
            status(r)
        );
    }
    Ok(())
}

fn extract_stored(client: &Path, part: u32, out: &Path) -> Result<()> {
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let root = fenji_data::re_dir::resolve_out_dir(&repo, &out.join(format!("data{part:03}")))?;
    let path = part_path(client, part);
    let h = read_part_header(&path).with_context(|| path.display().to_string())?;
    let mut f = File::open(&path)?;
    let (mut written, mut matching, mut skipped) = (0u64, 0u64, 0u64);
    for r in h.live() {
        let offset = u64::from(r.offset);
        match read_blob_header(&mut f, offset) {
            Ok(b) if b.codec == Codec::Stored => {
                let ext = match content_magic(&mut f, offset, b.raw_size)? {
                    Some(m) if &m == b"KB2j" => "bk2",
                    Some(m) if &m == b"RIFF" => "riff",
                    Some(m) if &m == CONTAINER_MAGIC => "sp",
                    _ => "bin",
                };
                let target = root.join(format!("{}.{ext}", hex(&r.key)));
                let sha1 = copy_out(&mut f, offset + FIXED_LEN as u64, b.raw_size, &target)?;
                written += 1;
                matching += u64::from(sha1 == r.key);
            }
            _ => skipped += 1,
        }
    }
    eprintln!(
        "data{part:03}: {written} stored files written to {}; {matching} match their key (the SHA-1 of the content), {} don't; {skipped} compressed or unreadable skipped",
        root.display(),
        written - matching
    );
    Ok(())
}

/// A new file at `target`. Whatever is there already (a symlink or hard link included) is replaced,
/// never written through.
fn create_replacing(target: &Path) -> Result<File> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => std::fs::remove_file(target)
            .with_context(|| format!("replacing {}", target.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    File::options()
        .write(true)
        .create_new(true)
        .open(target)
        .with_context(|| format!("writing {}", target.display()))
}

/// Copies `len` bytes at `at` to a new file at `target` and returns their SHA-1. Whatever is at
/// `target` already (a symlink or hard link included) is replaced, never written through.
fn copy_out(f: &mut File, at: u64, len: u64, target: &Path) -> Result<[u8; 20]> {
    let mut out = create_replacing(target)?;
    f.seek(SeekFrom::Start(at))?;
    let (mut hasher, mut buf, mut left) = (Sha1::new(), vec![0; 1 << 20], len);
    while left > 0 {
        let n = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        f.read_exact(&mut buf[..n])?;
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        left -= n as u64;
    }
    Ok(hasher.finalize().into())
}
