//! archive: the SoLO data archives' structure (M1 spec §5, docs/re/m1-archives.md). Oodle (codec 4)
//! is read only through the spool that scripts/spike/run-oodle-helper.sh decodes (M2 plan Task 11):
//! `paths` decodes `_index` that way, `dump` the design tables' cache (docs/re/m2-tables.md), and
//! `extract --key` any one file; `extract --part` writes a part's stored files. Writes only under
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
use fenji_data::{Codec, FIXED_LEN, cache, index};
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
    /// Write one part's stored (uncompressed) files under OUT/data0NN, named by key; or, with
    /// --key, the one file a live record holds under that key, decoded, as OUT/KEY
    ///
    /// With --key the file may be Oodle: its blocks go through the spool, and exit 3 means they
    /// wait for scripts/spike/run-oodle-helper.sh (not a gate event). OUT and the spool must be
    /// inside re/.
    Extract {
        #[arg(long, required_unless_present = "key", conflicts_with = "key")]
        part: Option<u32>,
        #[arg(long, conflicts_with = "key")]
        stored_only: bool,
        /// A file's key: 40 hex digits, as `_index` and `list` print it
        #[arg(long)]
        key: Option<String>,
        #[arg(long, default_value = "re/oodle-spool")]
        spool: PathBuf,
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
    /// Write one sheet of the design tables' cache as OUT/TABLE.jsonl, one row per line
    ///
    /// TABLE is items, skills, monsters, xp or maps (the sheets items, skill, npcs, upgrade and
    /// maps; docs/re/m2-tables.md §3), or any sheet's name, such as new_npcs. Each line is a JSON
    /// object: "$key", then the row's own fields; an `__index` that names the row itself is left
    /// out. OUT and the spool must be inside re/. Exit 3: blocks wait for
    /// scripts/spike/run-oodle-helper.sh (not a gate event).
    Dump {
        #[arg(long)]
        table: String,
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
        Command::Dump { table, spool, out } => return dump(&client, &table, &spool, &out),
        Command::Extract {
            key: Some(key),
            spool,
            out,
            ..
        } => return extract_key(&client, &key, &spool, &out),
        Command::Extract {
            part: Some(part),
            stored_only,
            out,
            ..
        } => {
            if !stored_only {
                bail!(
                    "with --part, only --stored-only is available: --key reads one file through the spool, Oodle included"
                );
            }
            extract_stored(&client, part, &out)
        }
        Command::Extract { .. } => bail!("pass --part or --key"),
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

/// The output directory and the spool, both inside re/ (the spool holds the client's compressed
/// blocks).
fn out_and_spool(out: &Path, spool: &Path) -> Result<(PathBuf, OodleSpool)> {
    let repo = fenji_data::re_dir::find_repo_root(&std::env::current_dir()?)?;
    let out = fenji_data::re_dir::resolve_out_dir(&repo, out)?;
    let spool = OodleSpool::new(&fenji_data::re_dir::resolve_out_dir(&repo, spool)?)?;
    Ok((out, spool))
}

/// A blob decoded through the spool, or the exit status (3) to stop with while its blocks wait for
/// the helper.
fn decode_spooled(
    bytes: &[u8],
    at: u64,
    spool: &OodleSpool,
) -> Result<std::result::Result<Vec<u8>, ExitCode>> {
    let queued = spool.spool_blob(bytes, at)?;
    if queued > 0 {
        println!("{queued} blocks spooled: run scripts/spike/run-oodle-helper.sh, then re-run");
        return Ok(Err(ExitCode::from(3)));
    }
    match fenji_data::decode_blob(bytes, at, spool) {
        Ok(raw) => Ok(Ok(raw)),
        Err(fenji_data::Error::OodlePending(name)) => {
            println!(
                "{name} and maybe more wait in the spool: run scripts/spike/run-oodle-helper.sh, then re-run"
            );
            Ok(Err(ExitCode::from(3)))
        }
        Err(e) => Err(e.into()),
    }
}

/// `_index` decoded through the spool, or the exit status to stop with.
fn decoded_index(
    client: &Path,
    spool: &OodleSpool,
) -> Result<std::result::Result<index::PathMap, ExitCode>> {
    let index_path = client.join("data").join("_index");
    let bytes = std::fs::read(&index_path).with_context(|| index_path.display().to_string())?;
    Ok(decode_spooled(&bytes, 0, spool)?.map(|raw| index::build(&raw)))
}

/// Every live record's place by key (the first part's wins), with counts for `paths`' summary.
struct Places {
    by_key: HashMap<[u8; 20], Place>,
    live: usize,
    repeated: usize,
    unreadable: usize,
}

fn live_places(client: &Path) -> Result<Places> {
    let mut p = Places {
        by_key: HashMap::new(),
        live: 0,
        repeated: 0,
        unreadable: 0,
    };
    for n in 0..1000 {
        let path = part_path(client, n);
        if !path.exists() {
            break;
        }
        let h = read_part_header(&path).with_context(|| path.display().to_string())?;
        let mut f = File::open(&path)?;
        for r in h.live() {
            p.live += 1;
            if p.by_key.contains_key(&r.key) {
                p.repeated += 1;
                continue;
            }
            let codec = match read_blob_header(&mut f, u64::from(r.offset)) {
                Ok(b) => b.codec.name(),
                Err(_) => {
                    p.unreadable += 1;
                    "bad-header"
                }
            };
            p.by_key.insert(
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
    Ok(p)
}

/// A live record's blob, read whole once it is known to lie inside its part.
fn read_record(client: &Path, p: &Place) -> Result<Vec<u8>> {
    let part = part_path(client, p.part);
    let mut f = File::open(&part).with_context(|| part.display().to_string())?;
    let len = f.metadata()?.len();
    if u64::from(p.offset) + u64::from(p.size) > len {
        bail!(
            "the record at {:#x} ({} bytes) runs past the end of {} ({len} bytes)",
            p.offset,
            p.size,
            part.display()
        );
    }
    f.seek(SeekFrom::Start(u64::from(p.offset)))?;
    let mut blob = vec![0; p.size as usize];
    f.read_exact(&mut blob)
        .with_context(|| part.display().to_string())?;
    Ok(blob)
}

/// A key's 20 bytes, from its 40 hex digits.
fn parse_key(key: &str) -> Result<[u8; 20]> {
    let digits = key.as_bytes();
    if digits.len() != 40 || !digits.iter().all(u8::is_ascii_hexdigit) {
        bail!("a key is 40 hex digits: {key:?}");
    }
    let mut k = [0; 20];
    for (b, pair) in k.iter_mut().zip(digits.as_chunks::<2>().0) {
        *b = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(k)
}

/// Writes the file a live record holds under `key` as OUT/KEY, decoded through the spool, and says
/// whether its SHA-1 is the key. Exits 3 when blocks wait for the helper.
fn extract_key(client: &Path, key: &str, spool: &Path, out: &Path) -> Result<ExitCode> {
    let k = parse_key(key)?;
    let (out, spool) = out_and_spool(out, spool)?;
    let places = live_places(client)?;
    let p = places
        .by_key
        .get(&k)
        .with_context(|| format!("no live record holds {}", hex(&k)))?;
    let blob = read_record(client, p)?;
    let raw = match decode_spooled(&blob, u64::from(p.offset), &spool)? {
        Ok(raw) => raw,
        Err(code) => return Ok(code),
    };
    let target = out.join(hex(&k));
    create_replacing(&target)?.write_all(&raw)?;
    let sha1: [u8; 20] = Sha1::digest(&raw).into();
    println!(
        "wrote {} bytes of {} (data{:03}, {}) to {}; its SHA-1 {} the key",
        raw.len(),
        hex(&k),
        p.part,
        p.codec,
        target.display(),
        if sha1 == k {
            "matches"
        } else {
            "doesn't match"
        }
    );
    Ok(ExitCode::SUCCESS)
}

/// Decodes `_index` through the spool, then writes OUT/paths.tsv. Exits 3 when blocks wait for the
/// helper.
fn paths(client: &Path, spool: &Path, out: &Path) -> Result<ExitCode> {
    let (out, spool) = out_and_spool(out, spool)?;
    let map = match decoded_index(client, &spool)? {
        Ok(map) => map,
        Err(code) => return Ok(code),
    };
    let places = live_places(client)?;
    let tsv = out.join("paths.tsv");
    let mut w = BufWriter::new(create_replacing(&tsv)?);
    writeln!(w, "key\tpath\tpart\toffset\tsize\tcodec")?;
    let mut found = 0usize;
    for (path, key) in &map.by_path {
        match places.by_key.get(key) {
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
        "{} paths ({} containers); {} bad lines, {} lines not UTF-8, {} paths named again; {found} found in a part, {} not found; {} live records, {} repeating a key, {} with an unreadable blob header; wrote {}",
        map.by_path.len(),
        map.containers.len(),
        map.bad_lines,
        map.non_utf8_lines,
        map.replaced,
        map.by_path.len() - found,
        places.live,
        places.repeated,
        places.unreadable,
        tsv.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// The design tables' cache (docs/re/m2-tables.md §1).
const TABLE_CACHE: &str = "data/caches/all_tables.new_cache";

/// `dump`'s names for the five sheets M2 dumps (docs/re/m2-tables.md §3); any other name is a
/// sheet's own.
fn sheet_name(table: &str) -> &str {
    match table {
        "skills" => "skill",
        "monsters" => "npcs",
        "xp" => "upgrade",
        other => other,
    }
}

/// Writes one sheet of the table cache as OUT/TABLE.jsonl. Exits 3 when blocks wait for the helper.
fn dump(client: &Path, table: &str, spool: &Path, out: &Path) -> Result<ExitCode> {
    // The name becomes a file name.
    if table.is_empty()
        || !table
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        bail!("a table name is letters, digits and '_': {table:?}");
    }
    let (out, spool) = out_and_spool(out, spool)?;
    let map = match decoded_index(client, &spool)? {
        Ok(map) => map,
        Err(code) => return Ok(code),
    };
    let key = map
        .by_path
        .get(TABLE_CACHE)
        .with_context(|| format!("_index doesn't name {TABLE_CACHE}"))?;
    let places = live_places(client)?;
    let p = places
        .by_key
        .get(key)
        .with_context(|| format!("no live record holds {TABLE_CACHE}"))?;
    let blob = read_record(client, p).with_context(|| TABLE_CACHE.to_string())?;
    let raw = match decode_spooled(&blob, u64::from(p.offset), &spool)? {
        Ok(raw) => raw,
        Err(code) => return Ok(code),
    };
    // A file's key is the SHA-1 of its decoded content (m2-tables.md 1.6), so a mismatch means a
    // damaged archive or a bad spool output, not the client's tables.
    let sha1: [u8; 20] = Sha1::digest(&raw).into();
    if sha1 != *key {
        bail!(
            "{TABLE_CACHE} decoded to {} bytes whose SHA-1 isn't its key {}: a damaged archive or a bad spool output",
            raw.len(),
            hex(key)
        );
    }
    let unzipped = cache::unzip(&raw)?;
    let c = cache::read(unzipped.as_deref().unwrap_or(&raw))?;
    let name = sheet_name(table);
    let id = c
        .sheet(name)
        .with_context(|| format!("the table cache has no sheet {name:?}"))?;
    let rows = cache::json::rows(&c, id);
    let file = out.join(format!("{table}.jsonl"));
    let mut w = BufWriter::new(create_replacing(&file)?);
    for r in &rows {
        writeln!(w, "{r}")?;
    }
    w.flush()?;
    println!(
        "{} rows of {name} written to {}",
        rows.len(),
        file.display()
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
