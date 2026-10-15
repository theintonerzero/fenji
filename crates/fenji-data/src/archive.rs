//! The data archives' structure without decompression (M1 spec §5): each `data0NN` part's chain
//! of index blocks and their records, the blob headers that records point at, and the containers'
//! records. The layout is the engine's own reader's, FUN_1400bf780 and FUN_1400bff10
//! (docs/re/m1-archives.md). [`each_oodle_blob`] walks every Oodle blob of a client copy.
use std::collections::HashSet;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::{BlobHeader, Codec, Error, FIXED_LEN, Result};

/// A part's first four bytes.
pub const PART_MAGIC: &[u8; 4] = b"ZFS\0";
/// An index block's first four bytes.
pub const INDEX_MAGIC: &[u8; 4] = b"[IX]";
/// A container's first four bytes: a stored file that holds many others (FUN_1400beb00).
pub const CONTAINER_MAGIC: &[u8; 4] = b"[SP]";
/// A container's header (docs/re/m1-archives.md 4.3): [`CONTAINER_MAGIC`], then the file count,
/// the listing's length and the records' offset as `u32`s, then a CRC-16/ARC of those 12 bytes.
pub const CONTAINER_HEADER_LEN: usize = 18;
/// Records per index block.
pub const RECORDS: usize = 4096;
pub const RECORD_LEN: usize = 32;
/// An index block: its magic, the next block's offset (0 ends the chain), then the records.
pub const INDEX_BLOCK_LEN: usize = 8 + RECORDS * RECORD_LEN;
/// Where a part's first index block starts.
pub const FIRST_INDEX: u64 = PART_MAGIC.len() as u64;
/// The first byte after the first index block (0x2000C).
pub const DATA_START: u64 = FIRST_INDEX + INDEX_BLOCK_LEN as u64;
/// A free record: the engine stops reading its block there.
pub const STATUS_FREE: u8 = 0;
/// A record in use. Any other non-zero status (0xff in the client's parts) is a deleted record.
pub const STATUS_USED: u8 = 1;
/// More index blocks than this in one part means a corrupt chain. The client's longest has 6.
const MAX_INDEX_BLOCKS: usize = 1024;
/// More blocks than this in one blob means a corrupt header; no archive file comes near it.
const MAX_BLOCKS: u64 = 1 << 20;

/// One record of a part's index (field meanings: docs/re/m1-archives.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Position in the part's chain: block number × [`RECORDS`] + slot in the block.
    pub slot: u32,
    /// SHA-1 of the file's content.
    pub key: [u8; 20],
    pub offset: u32,
    pub size: u32,
    /// CRC-16/ARC over the record's first 28 bytes: key, offset and size.
    pub crc: u16,
    pub status: u8,
}

impl Record {
    fn parse(slot: u32, r: &[u8]) -> Self {
        let word = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().expect("4 bytes"));
        Self {
            slot,
            key: r[..20].try_into().expect("20 bytes"),
            offset: word(20),
            size: word(24),
            crc: u16::from_le_bytes([r[28], r[29]]),
            status: r[31],
        }
    }

    /// Whether the stored CRC matches the key, offset and size.
    pub fn crc_ok(&self) -> bool {
        let mut b = [0u8; 28];
        b[..20].copy_from_slice(&self.key);
        b[20..24].copy_from_slice(&self.offset.to_le_bytes());
        b[24..28].copy_from_slice(&self.size.to_le_bytes());
        crc16_arc(&b) == self.crc
    }

    /// What the engine loads: a record in use whose CRC is good.
    pub fn is_live(&self) -> bool {
        self.status == STATUS_USED && self.crc_ok()
    }
}

/// A part's index: where each block of its chain starts, and each block's records up to its first
/// free one (live, deleted and damaged alike), in chain order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartHeader {
    pub blocks: Vec<u64>,
    pub records: Vec<Record>,
}

impl PartHeader {
    /// The records the engine loads: in use, with a good CRC. The engine also drops a record whose
    /// key repeats one it has already loaded; no live key repeats in the pinned build's parts.
    pub fn live(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().filter(|r| r.is_live())
    }
}

/// CRC-16/ARC (reflected polynomial 0xA001, initial value 0): the engine's record checksum.
pub fn crc16_arc(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0, |crc, &b| {
        (0..8).fold(crc ^ u16::from(b), |c, _| {
            if c & 1 == 1 {
                (c >> 1) ^ 0xA001
            } else {
                c >> 1
            }
        })
    })
}

/// Reads a part's index: its magic, then each index block of the chain. Reads only the index
/// blocks, never the data around them.
pub fn read_part_header_from<R: Read + Seek>(r: &mut R) -> Result<PartHeader> {
    let mut magic = [0u8; 4];
    read_at(r, 0, &mut magic, "the part magic")?;
    if &magic != PART_MAGIC {
        return Err(Error::BadArchive("no ZFS\\0 magic".into()));
    }
    let (mut blocks, mut records, mut seen) = (Vec::new(), Vec::new(), HashSet::new());
    let mut block = vec![0u8; INDEX_BLOCK_LEN];
    let mut at = FIRST_INDEX;
    loop {
        if !seen.insert(at) {
            return Err(Error::BadArchive(format!(
                "the index chain loops back to {at:#x}"
            )));
        }
        if blocks.len() == MAX_INDEX_BLOCKS {
            return Err(Error::BadArchive(format!(
                "more than {MAX_INDEX_BLOCKS} index blocks"
            )));
        }
        read_at(r, at, &mut block, "an index block")?;
        if &block[..4] != INDEX_MAGIC {
            return Err(Error::BadArchive(format!("no [IX] magic at {at:#x}")));
        }
        let first_slot = (blocks.len() * RECORDS) as u32;
        blocks.push(at);
        records.extend(
            block[8..]
                .as_chunks::<RECORD_LEN>()
                .0
                .iter()
                .take_while(|rec| rec[31] != STATUS_FREE)
                .enumerate()
                .map(|(slot, rec)| Record::parse(first_slot + slot as u32, rec)),
        );
        match u32::from_le_bytes(block[4..8].try_into().expect("4 bytes")) {
            0 => return Ok(PartHeader { blocks, records }),
            next => at = u64::from(next),
        }
    }
}

/// Reads the index of the part at `path`.
pub fn read_part_header(path: &Path) -> Result<PartHeader> {
    read_part_header_from(&mut File::open(path)?)
}

/// Reads the index of a whole part held in memory.
pub fn parse_part_header(bytes: &[u8]) -> Result<PartHeader> {
    read_part_header_from(&mut Cursor::new(bytes))
}

/// The blob header at `offset`: the fixed part, plus the block list for compressed codecs. A
/// stored blob has none; its content follows the fixed part.
pub fn read_blob_header<R: Read + Seek>(r: &mut R, offset: u64) -> Result<BlobHeader> {
    let bad = |reason: String| Error::BadBlob { at: offset, reason };
    let past_end = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => bad("runs past the end of the file".into()),
        _ => Error::Io(e),
    };
    let mut head = vec![0; FIXED_LEN];
    r.seek(SeekFrom::Start(offset))?;
    r.read_exact(&mut head).map_err(past_end)?;
    let raw = u64::from_le_bytes(head[0..8].try_into().expect("8 bytes"));
    let block = u32::from_le_bytes(head[24..28].try_into().expect("4 bytes"));
    let codec = Codec::from_u16(u16::from_le_bytes([head[28], head[29]]));
    let blocks = match codec {
        Codec::Stored => 0,
        _ if raw == 0 || block == 0 => 0,
        _ => raw.div_ceil(u64::from(block)),
    };
    if blocks > MAX_BLOCKS {
        return Err(bad(format!("{blocks} blocks")));
    }
    head.resize(FIXED_LEN + 4 * blocks as usize, 0);
    r.read_exact(&mut head[FIXED_LEN..]).map_err(past_end)?;
    BlobHeader::parse(&head, offset)
}

/// The records of a container whose content (the stored file's bytes, from its `[SP]`) is
/// `content`, in the layout of docs/re/m1-archives.md 4.3: the header, a listing of
/// `<key>\t<path>` lines, then one record per file in the part layout, its offset counted from the
/// content's first byte. Each record's slot is its place in the list.
///
/// Every record must be live, as all of the pinned client's are, and its file must lie inside
/// `content`, so `content[offset..offset + size]` is in bounds for each. Anything else is a damaged
/// container, and an error.
pub fn container_records(content: &[u8]) -> Result<Vec<Record>> {
    let bad = |reason: &str| Error::BadArchive(format!("container: {reason}"));
    let head = content
        .get(..CONTAINER_HEADER_LEN)
        .ok_or_else(|| bad("shorter than its header"))?;
    if &head[..4] != CONTAINER_MAGIC {
        return Err(bad("no [SP] magic"));
    }
    if crc16_arc(&head[4..16]) != u16::from_le_bytes([head[16], head[17]]) {
        return Err(bad("its header's CRC doesn't check"));
    }
    let field = |o: usize| u32::from_le_bytes(head[o..o + 4].try_into().expect("4 bytes")) as usize;
    let (count, listing, records_at) = (field(4), field(8), field(12));
    if Some(records_at) != CONTAINER_HEADER_LEN.checked_add(listing) {
        return Err(bad("its records don't follow its listing"));
    }
    let records = count
        .checked_mul(RECORD_LEN)
        .and_then(|len| content.get(records_at..records_at.checked_add(len)?))
        .ok_or_else(|| bad(&format!("{count} records run past its end")))?;
    records
        .as_chunks::<RECORD_LEN>()
        .0
        .iter()
        .enumerate()
        .map(|(slot, r)| {
            let r = Record::parse(slot as u32, r);
            if !r.is_live() {
                return Err(bad(&format!("record {slot} isn't live")));
            }
            if u64::from(r.offset) + u64::from(r.size) > content.len() as u64 {
                return Err(bad(&format!("record {slot}'s file runs past its end")));
            }
            Ok(r)
        })
        .collect()
}

/// Where [`each_oodle_blob`] found an Oodle blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OodleSource {
    /// `_index` (docs/re/m1-archives.md 3.1).
    Index,
    /// A file inside a container (4.3).
    Container,
    /// A part's live record.
    Part,
}

/// Calls `each(source, name, blob)` with every Oodle blob of the client copy whose `data/` is
/// `data`, so that a check can cover every Oodle block (the Oodle decoder plan's Task 7, Ruling
/// O9). In order, with names that are unique:
/// - `_index`, named `_index`;
/// - every Oodle file inside the containers, each container's records in order, named
///   `<container key>/<slot>/<file key>`: every record is walked, so a key that two records share
///   is walked at each;
/// - the parts' live Oodle records, the first for each key as the engine loads them (1.5), named
///   by their key, as `archive`'s `live_places` finds them.
///
/// Every part's index chain and blob headers, every container's records and their files' blob
/// headers, and `_index`'s, are read first: a damaged part or container stops the walk before
/// `each` sees any blob. So do more than [`MAX_CONTAINERS`] containers. A walked part record's size
/// must be its blob's stored size (docs/re/m1-archives.md 2.2), and a container's file and `_index`
/// must hold their blobs whole. Each such error names the blob, or the part, it's about. One
/// container at a time is in memory. An error from `each` stops the walk too, and is handed back.
pub fn each_oodle_blob<E: From<Error>>(
    data: &Path,
    mut each: impl FnMut(OodleSource, &str, &[u8]) -> std::result::Result<(), E>,
) -> std::result::Result<(), E> {
    let (parts, containers) = oodle_places(data)?;
    let index_path = data.join("_index");
    let index = read_index(&index_path).map_err(|e| about(index_path.display(), e))?;
    if let Some(index) = index {
        each(OodleSource::Index, "_index", &index)?;
    }
    for c in &containers {
        let (content, oodle) = read_container(c)?;
        for r in &oodle {
            // In bounds: `container_records` checks every record's file.
            let file = &content[r.offset as usize..][..r.size as usize];
            let name = format!("{}/{}/{}", crate::hex(&c.key), r.slot, crate::hex(&r.key));
            each(OodleSource::Container, &name, file)?;
        }
    }
    for (path, oodle) in &parts {
        let mut f = File::open(path).map_err(|e| about(path.display(), e.into()))?;
        for &(key, at, size) in oodle {
            let what = || format!("{} at {at:#x} in {}", crate::hex(&key), path.display());
            let blob = read_blob(&mut f, at, size).map_err(|e| about(what(), e))?;
            each(OodleSource::Part, &crate::hex(&key), &blob)?;
        }
    }
    Ok(())
}

/// `e`, said of `what`: a blob or a part, by its key or its path.
fn about(what: impl std::fmt::Display, e: Error) -> Error {
    match e {
        Error::BadArchive(e) => Error::BadArchive(format!("{what}: {e}")),
        e => Error::BadArchive(format!("{what}: {e}")),
    }
}

/// `_index` whole, if it is an Oodle blob.
fn read_index(path: &Path) -> Result<Option<Vec<u8>>> {
    let index = std::fs::read(path)?;
    let h = BlobHeader::parse(&index, 0)?;
    if h.stored_size > index.len() as u64 {
        return Err(Error::BadArchive(format!(
            "its stored size, {}, is more than its {} bytes",
            h.stored_size,
            index.len()
        )));
    }
    Ok((h.codec == Codec::Oodle).then_some(index))
}

/// More containers than this among the parts' live records means a damaged archive: the pinned
/// client has 2 (docs/re/m1-archives.md 4.3).
pub const MAX_CONTAINERS: usize = 16;

/// A part, and its live Oodle records' keys, offsets and sizes, for [`each_oodle_blob`].
type PartPlaces = (PathBuf, Vec<([u8; 20], u64, u32)>);

/// A container found among the parts' live records: its part, key, offset and size. Its bytes are
/// read when they're needed, so one container at a time is in memory.
struct ContainerPlace {
    part: PathBuf,
    key: [u8; 20],
    at: u64,
    size: u32,
}

/// The first pass of [`each_oodle_blob`]: every part's index chain and blob headers, giving each
/// part's live Oodle records, the first for each key, and every container, each checked.
fn oodle_places(data: &Path) -> Result<(Vec<PartPlaces>, Vec<ContainerPlace>)> {
    let (mut parts, mut containers) = (Vec::new(), Vec::new());
    let mut seen = HashSet::new();
    for n in 0..1000 {
        let path = data.join(format!("data{n:03}"));
        if !path.exists() {
            break;
        }
        let h = read_part_header(&path).map_err(|e| about(path.display(), e))?;
        let mut f = File::open(&path).map_err(|e| about(path.display(), e.into()))?;
        let len = f
            .metadata()
            .map_err(|e| about(path.display(), e.into()))?
            .len();
        let mut oodle = Vec::new();
        for r in h.live() {
            if !seen.insert(r.key) {
                continue;
            }
            let at = u64::from(r.offset);
            let what = || format!("{} at {at:#x} in {}", crate::hex(&r.key), path.display());
            let b = read_blob_header(&mut f, at).map_err(|e| about(what(), e))?;
            if b.codec == Codec::Oodle {
                check_record(r, &b, len).map_err(|e| about(what(), e))?;
                oodle.push((r.key, at, r.size));
                continue;
            }
            if b.codec != Codec::Stored || b.raw_size < 4 {
                continue;
            }
            let mut magic = [0; 4];
            read_at(&mut f, at + FIXED_LEN as u64, &mut magic, "a stored file")
                .map_err(|e| about(what(), e))?;
            if &magic != CONTAINER_MAGIC {
                continue;
            }
            check_record(r, &b, len).map_err(|e| about(what(), e))?;
            if containers.len() == MAX_CONTAINERS {
                return Err(about(
                    what(),
                    Error::BadArchive(format!(
                        "more than {MAX_CONTAINERS} containers, which means a damaged archive \
                         (the pinned client has 2)"
                    )),
                ));
            }
            let place = ContainerPlace {
                part: path.clone(),
                key: r.key,
                at,
                size: r.size,
            };
            read_container(&place)?;
            containers.push(place);
        }
        parts.push((path, oodle));
    }
    Ok((parts, containers))
}

/// A part record the walk reads lies inside its part, of `len` bytes, and its size is its blob's
/// stored size, as for every live record of the pinned client (docs/re/m1-archives.md 2.2).
fn check_record(r: &Record, b: &BlobHeader, len: u64) -> Result<()> {
    if u64::from(r.offset) + u64::from(r.size) > len {
        return Err(Error::BadArchive(format!(
            "its {} bytes run past the end of its part, at {len}",
            r.size
        )));
    }
    if u64::from(r.size) != b.stored_size {
        return Err(Error::BadArchive(format!(
            "its size, {}, isn't its blob's stored size, {}",
            r.size, b.stored_size
        )));
    }
    Ok(())
}

/// A container's content, and the records of its Oodle files, checked: its records as
/// [`container_records`] checks them, and each file's blob, whose header must parse and whose
/// stored size its record must hold. An error names the container, and the file if it's one's.
fn read_container(c: &ContainerPlace) -> Result<(Vec<u8>, Vec<Record>)> {
    let what = format!(
        "{} at {:#x} in {}",
        crate::hex(&c.key),
        c.at,
        c.part.display()
    );
    let mut f = File::open(&c.part).map_err(|e| about(&what, e.into()))?;
    let mut content = read_blob(&mut f, c.at, c.size).map_err(|e| about(&what, e))?;
    let h = BlobHeader::parse(&content, c.at).map_err(|e| about(&what, e))?;
    if h.codec != Codec::Stored || h.stored_size > u64::from(c.size) {
        let e = Error::BadArchive("not a whole stored blob".into());
        return Err(about(&what, e));
    }
    // The stored blob's data, in place: its content, from the `[SP]`.
    content.truncate(h.stored_size as usize);
    content.drain(..h.header_len());
    let mut oodle = Vec::new();
    for file in container_records(&content).map_err(|e| about(&what, e))? {
        let file_what = || format!("{what}, its file {} ({})", file.slot, crate::hex(&file.key));
        // In bounds: `container_records` checks every record's file.
        let bytes = &content[file.offset as usize..][..file.size as usize];
        let fh =
            BlobHeader::parse(bytes, u64::from(file.offset)).map_err(|e| about(file_what(), e))?;
        if fh.stored_size > u64::from(file.size) {
            let e = Error::BadArchive(format!(
                "its blob's stored size, {}, is more than its record's {} bytes",
                fh.stored_size, file.size
            ));
            return Err(about(file_what(), e));
        }
        if fh.codec == Codec::Oodle {
            oodle.push(file);
        }
    }
    Ok((content, oodle))
}

/// The `size` bytes of the blob at `at`, once they're known to lie inside the file.
fn read_blob(f: &mut File, at: u64, size: u32) -> Result<Vec<u8>> {
    let len = f.metadata()?.len();
    if at + u64::from(size) > len {
        return Err(Error::BadArchive(format!(
            "its {size} bytes run past the end of its part, at {len}"
        )));
    }
    let mut blob = vec![0; size as usize];
    read_at(f, at, &mut blob, "a record's blob")?;
    Ok(blob)
}

fn read_at<R: Read + Seek>(r: &mut R, at: u64, buf: &mut [u8], what: &str) -> Result<()> {
    r.seek(SeekFrom::Start(at))?;
    r.read_exact(buf).map_err(|e| match e.kind() {
        std::io::ErrorKind::UnexpectedEof => {
            Error::BadArchive(format!("{what} at {at:#x} runs past the end of the file"))
        }
        _ => Error::Io(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Codec;
    use crate::test_util::{
        container, lz4_blob, oodle_blob as oodle_file, oodle_memset_blob, stored_blob, zfs_part,
    };

    /// An LZ4 blob relabelled codec 4: its header is all these tests read.
    fn oodle_blob() -> Vec<u8> {
        let mut b = lz4_blob(b"abcabcabcabc", 0);
        b[28] = 4;
        b
    }

    /// Two index blocks: the first holds a stored file and an Oodle file, the second a deleted
    /// record and a container.
    fn part() -> Vec<u8> {
        zfs_part(&[
            vec![
                (1, [1; 20], stored_blob(b"KB2j fake video bytes")),
                (1, [2; 20], oodle_blob()),
            ],
            vec![
                (0xff, [3; 20], stored_blob(b"old")),
                (1, [4; 20], stored_blob(b"[SP] container")),
            ],
        ])
    }

    #[test]
    fn follows_the_index_chain() {
        let h = parse_part_header(&part()).unwrap();
        assert_eq!(h.blocks.len(), 2);
        assert_eq!(h.blocks[0], FIRST_INDEX);
        let slots: Vec<(u32, u8)> = h.records.iter().map(|r| (r.slot, r.status)).collect();
        let second = RECORDS as u32;
        assert_eq!(slots, [(0, 1), (1, 1), (second, 0xff), (second + 1, 1)]);
        assert_eq!(u64::from(h.records[0].offset), DATA_START);
        assert_eq!(h.records[1].key, [2; 20]);
        assert_eq!(h.live().count(), 3);
    }

    #[test]
    fn reads_blob_headers_at_record_offsets() {
        let bytes = part();
        let h = parse_part_header(&bytes).unwrap();
        let mut cur = std::io::Cursor::new(&bytes);
        let first = read_blob_header(&mut cur, u64::from(h.records[0].offset)).unwrap();
        let second = read_blob_header(&mut cur, u64::from(h.records[1].offset)).unwrap();
        assert_eq!(
            (first.codec, first.stored_size),
            (Codec::Stored, u64::from(h.records[0].size))
        );
        assert!(
            first.block_sizes.is_empty(),
            "a stored blob has no block list"
        );
        assert_eq!((second.codec, second.block_sizes.len()), (Codec::Oodle, 1));
    }

    #[test]
    fn reads_a_part_file_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data000");
        std::fs::write(&path, part()).unwrap();
        assert_eq!(
            read_part_header(&path).unwrap(),
            parse_part_header(&part()).unwrap()
        );
    }

    #[test]
    fn crc16_is_the_arc_variant() {
        assert_eq!(crc16_arc(b"123456789"), 0xBB3D);
    }

    #[test]
    fn a_record_with_a_bad_crc_is_not_live() {
        let mut bytes = part();
        let first = FIRST_INDEX as usize + 8;
        bytes[first + 28] ^= 1;
        let h = parse_part_header(&bytes).unwrap();
        assert!(!h.records[0].crc_ok() && !h.records[0].is_live());
        assert!(h.records[1].crc_ok());
        assert_eq!(h.live().count(), 2);
    }

    #[test]
    fn a_free_record_ends_its_block() {
        // The engine stops reading a block at its first free record; the chain goes on.
        let mut bytes = part();
        let first = FIRST_INDEX as usize + 8;
        bytes[first + 31] = STATUS_FREE;
        let h = parse_part_header(&bytes).unwrap();
        assert!(h.records.iter().all(|r| r.slot >= RECORDS as u32), "{h:?}");
        assert_eq!(h.records.len(), 2);
    }

    #[test]
    fn refuses_what_isnt_a_part() {
        for bytes in [&b"ZIP\0"[..], b"ZFS\0[IX]", b"ZF"] {
            assert!(
                matches!(parse_part_header(bytes), Err(Error::BadArchive(_))),
                "{bytes:?}"
            );
        }
        let mut no_index = part();
        no_index[FIRST_INDEX as usize] = b'?';
        assert!(matches!(
            parse_part_header(&no_index),
            Err(Error::BadArchive(_))
        ));
    }

    #[test]
    fn refuses_a_chain_that_loops_or_leaves_the_file() {
        let next = FIRST_INDEX as usize + 4;
        let mut looping = part();
        let second = u32::from_le_bytes(looping[next..next + 4].try_into().unwrap()) as usize;
        looping[second + 4..second + 8].copy_from_slice(&(FIRST_INDEX as u32).to_le_bytes());
        assert!(
            matches!(parse_part_header(&looping), Err(Error::BadArchive(ref m)) if m.contains("loop"))
        );
        let mut outside = part();
        outside[next..next + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse_part_header(&outside),
            Err(Error::BadArchive(_))
        ));
    }

    #[test]
    fn refuses_an_implausible_block_list_before_reading_it() {
        let mut blob = oodle_blob();
        blob[..8].copy_from_slice(&(1u64 << 31).to_le_bytes()); // raw size 2 GiB …
        blob[24..28].copy_from_slice(&1u32.to_le_bytes()); // … in 1-byte blocks
        let mut cur = std::io::Cursor::new(&blob);
        assert!(matches!(
            read_blob_header(&mut cur, 0),
            Err(Error::BadBlob { .. })
        ));
    }

    #[test]
    fn a_blob_header_past_the_end_is_an_error_not_a_panic() {
        let blob = stored_blob(b"x");
        let mut cur = std::io::Cursor::new(&blob[..20]);
        assert!(matches!(
            read_blob_header(&mut cur, 0),
            Err(Error::BadBlob { .. })
        ));
    }

    /// A container of two Oodle files and a stored one.
    fn two_files_and_a_note() -> Vec<u8> {
        container(&[
            ([0xA1; 20], "asset/a.dds", oodle_file(b"first file", 512)),
            ([0xA2; 20], "asset/b.dds", oodle_memset_blob(1000, 512, 9)),
            ([0xA3; 20], "asset/note.txt", stored_blob(b"a note")),
        ])
    }

    #[test]
    fn reads_a_containers_records() {
        let content = two_files_and_a_note();
        let records = container_records(&content).unwrap();
        let keys: Vec<[u8; 20]> = records.iter().map(|r| r.key).collect();
        assert_eq!(keys, [[0xA1; 20], [0xA2; 20], [0xA3; 20]]);
        assert_eq!(records[1].slot, 1);
        // Each record's offset counts from the content's first byte, the `[SP]`.
        let file = |r: &Record| &content[r.offset as usize..][..r.size as usize];
        assert_eq!(file(&records[0]), oodle_file(b"first file", 512));
        assert_eq!(file(&records[2]), stored_blob(b"a note"));
    }

    #[test]
    fn refuses_a_container_that_doesnt_check() {
        let good = two_files_and_a_note();
        let refused = |bytes: &[u8], what: &str| {
            assert!(
                matches!(container_records(bytes), Err(Error::BadArchive(_))),
                "{what}"
            );
        };
        let mut c = good.clone();
        c[1] = b'X';
        refused(&c, "no [SP] magic");
        let mut c = good.clone();
        c[16] ^= 1;
        refused(&c, "a header CRC that doesn't check");
        // A records' offset that isn't 18 + the listing's length, its CRC made good again.
        let mut c = good.clone();
        let at = u32::from_le_bytes(c[12..16].try_into().unwrap()) + 1;
        c[12..16].copy_from_slice(&at.to_le_bytes());
        let crc = crc16_arc(&c[4..16]);
        c[16..18].copy_from_slice(&crc.to_le_bytes());
        refused(&c, "records that don't follow the listing");
        // More files than records fit, its CRC made good again.
        let mut c = good.clone();
        c[4..8].copy_from_slice(&1000u32.to_le_bytes());
        let crc = crc16_arc(&c[4..16]);
        c[16..18].copy_from_slice(&crc.to_le_bytes());
        refused(&c, "records past the end");
        // A record that isn't live: the engine wouldn't load it, and the client has none.
        let first = CONTAINER_HEADER_LEN + good[8] as usize;
        let mut c = good.clone();
        c[first + 31] = 0xFF;
        refused(&c, "a deleted record");
        let mut c = good.clone();
        c[first] ^= 1;
        refused(&c, "a record whose CRC doesn't check");
        // A record whose file runs past the content, its CRC made good again.
        let mut c = good.clone();
        let size = u32::from_le_bytes(c[first + 24..first + 28].try_into().unwrap()) + 10_000;
        c[first + 24..first + 28].copy_from_slice(&size.to_le_bytes());
        let crc = crc16_arc(&c[first..first + 28]);
        c[first + 28..first + 30].copy_from_slice(&crc.to_le_bytes());
        refused(&c, "a file past the end");
        // Cut short anywhere before its last file: an error, never a panic.
        for n in 0..good.len() - 1 {
            assert!(container_records(&good[..n]).is_err(), "cut at {n}");
        }
    }

    /// A client copy's `data/` with `_index`, a container and two parts. data000 holds a stored
    /// file, an Oodle file, a deleted Oodle record and the container; data001 repeats the Oodle
    /// file's key and adds another.
    fn client_data() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("_index"), oodle_file(b"the index", 4)).unwrap();
        let part0 = zfs_part(&[vec![
            (1, [1; 20], stored_blob(b"plain")),
            (1, [2; 20], oodle_file(b"two", 512)),
            (0xff, [3; 20], oodle_file(b"deleted", 512)),
            (1, [4; 20], stored_blob(&two_files_and_a_note())),
        ]]);
        std::fs::write(dir.path().join("data000"), part0).unwrap();
        let part1 = zfs_part(&[vec![
            (1, [2; 20], oodle_file(b"two, again", 512)),
            (1, [5; 20], oodle_memset_blob(700_000, 512 * 1024, 5)),
        ]]);
        std::fs::write(dir.path().join("data001"), part1).unwrap();
        dir
    }

    #[test]
    fn walks_every_oodle_blob_of_a_client_copy() {
        let data = client_data();
        let mut seen = Vec::new();
        each_oodle_blob(data.path(), |source, name, blob| {
            seen.push((source, name.to_string(), blob.to_vec()));
            Ok::<(), Error>(())
        })
        .unwrap();
        let c = "04".repeat(20);
        assert_eq!(
            seen,
            [
                (
                    OodleSource::Index,
                    "_index".to_string(),
                    oodle_file(b"the index", 4)
                ),
                (
                    OodleSource::Container,
                    format!("{c}/0/{}", "a1".repeat(20)),
                    oodle_file(b"first file", 512)
                ),
                (
                    OodleSource::Container,
                    format!("{c}/1/{}", "a2".repeat(20)),
                    oodle_memset_blob(1000, 512, 9)
                ),
                (OodleSource::Part, "02".repeat(20), oodle_file(b"two", 512)),
                (
                    OodleSource::Part,
                    "05".repeat(20),
                    oodle_memset_blob(700_000, 512 * 1024, 5)
                ),
            ]
        );
    }

    #[test]
    fn a_damaged_container_stops_the_walk_before_any_blob() {
        let mut header = two_files_and_a_note();
        header[16] ^= 1;
        let file = container(&[([0xA4; 20], "asset/c.dds", b"not a blob".to_vec())]);
        for content in [header, file] {
            let data = client_data();
            let part = zfs_part(&[vec![(1, [6; 20], stored_blob(&content))]]);
            std::fs::write(data.path().join("data002"), part).unwrap();
            let mut called = false;
            let r = each_oodle_blob(data.path(), |_, _, _| {
                called = true;
                Ok::<(), Error>(())
            });
            assert!(
                matches!(r, Err(Error::BadArchive(ref m)) if m.contains(&"06".repeat(20))),
                "{r:?}"
            );
            assert!(!called);
        }
    }

    #[test]
    fn the_walk_hands_back_the_callers_error() {
        let data = client_data();
        let r = each_oodle_blob(data.path(), |_, name, _| {
            if name == "_index" {
                Err(Error::OutDir("stop".into()))
            } else {
                Ok(())
            }
        });
        assert!(
            matches!(r, Err(Error::OutDir(ref m)) if m == "stop"),
            "{r:?}"
        );
    }

    /// A client copy's `data/` with a small `_index` and one part, `part`.
    fn data_with(part: Vec<u8>) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("_index"), oodle_file(b"the index", 4)).unwrap();
        std::fs::write(dir.path().join("data000"), part).unwrap();
        dir
    }

    /// A part whose index names one blob, `blob`, under each of `keys`.
    fn one_blob_under_many_keys(keys: &[[u8; 20]], blob: &[u8]) -> Vec<u8> {
        let records = keys
            .iter()
            .enumerate()
            .map(|(i, &key)| (1, key, if i == 0 { blob.to_vec() } else { Vec::new() }))
            .collect();
        let mut part = zfs_part(&[records]);
        let first = FIRST_INDEX as usize + 8;
        let place: [u8; 8] = part[first + 20..first + 28].try_into().unwrap();
        for i in 1..keys.len() {
            let r = first + i * RECORD_LEN;
            part[r + 20..r + 28].copy_from_slice(&place);
            let crc = crc16_arc(&part[r..r + 28]);
            part[r + 28..r + 30].copy_from_slice(&crc.to_le_bytes());
        }
        part
    }

    /// Sets the size of record `slot` of `records`, the first record of an index block or of a
    /// container's records, and makes its CRC good again.
    fn set_size(records: &mut [u8], slot: usize, size: impl Fn(u32) -> u32) {
        let r = slot * RECORD_LEN;
        let old = u32::from_le_bytes(records[r + 24..r + 28].try_into().unwrap());
        records[r + 24..r + 28].copy_from_slice(&size(old).to_le_bytes());
        let crc = crc16_arc(&records[r..r + 28]);
        records[r + 28..r + 30].copy_from_slice(&crc.to_le_bytes());
    }

    #[test]
    fn every_error_of_the_walk_names_its_blob() {
        // Each is caught in the first pass, before any blob is handed out.
        let refused = |data: &Path, name: &str, what: &str| {
            let mut called = false;
            let r = each_oodle_blob(data, |_, _, _| {
                called = true;
                Ok::<(), Error>(())
            });
            assert!(
                matches!(&r, Err(Error::BadArchive(m)) if m.contains(name)),
                "{what}: {r:?}"
            );
            assert!(!called, "{what}");
        };
        let records = FIRST_INDEX as usize + 8;
        let two = || {
            zfs_part(&[vec![
                (1, [2; 20], oodle_file(b"two", 512)),
                (1, [3; 20], stored_blob(b"x")),
            ]])
        };
        let mut part = two();
        set_size(&mut part[records..], 0, |n| n + 1);
        refused(
            data_with(part).path(),
            &"02".repeat(20),
            "a size that isn't the stored size",
        );
        let mut last = zfs_part(&[vec![(1, [4; 20], oodle_file(b"four", 512))]]);
        set_size(&mut last[records..], 0, |n| n + 100);
        refused(
            data_with(last).path(),
            &"04".repeat(20),
            "a record past its part's end",
        );
        let part = zfs_part(&[vec![(1, [5; 20], vec![0; 32])]]);
        refused(
            data_with(part).path(),
            &"05".repeat(20),
            "a blob header that doesn't parse",
        );
        refused(
            data_with(b"junk".to_vec()).path(),
            "data000",
            "a part that isn't one",
        );
        // A container's file whose blob runs past its record.
        let mut content = two_files_and_a_note();
        let first = CONTAINER_HEADER_LEN + content[8] as usize;
        set_size(&mut content[first..], 0, |n| n - 1);
        let part = zfs_part(&[vec![(1, [6; 20], stored_blob(&content))]]);
        refused(
            data_with(part).path(),
            &"a1".repeat(20),
            "a container's file past its record",
        );
        // _index, missing or cut short.
        let data = client_data();
        std::fs::remove_file(data.path().join("_index")).unwrap();
        refused(data.path(), "_index", "no _index");
        let data = client_data();
        let mut index = oodle_file(b"the index", 4);
        index.pop();
        std::fs::write(data.path().join("_index"), index).unwrap();
        refused(data.path(), "_index", "an _index cut short");
    }

    #[test]
    fn records_naming_one_container_are_bounded() {
        // Each key's record is a container to walk, held one at a time. Past 16 (the pinned
        // client has 2), the archive is damaged: refused before any blob.
        let content = stored_blob(&two_files_and_a_note());
        let keys: Vec<[u8; 20]> = (0..17u8).map(|i| [0x40 + i; 20]).collect();
        let data = data_with(one_blob_under_many_keys(&keys[..16], &content));
        let mut blobs = 0;
        each_oodle_blob(data.path(), |_, _, _| {
            blobs += 1;
            Ok::<(), Error>(())
        })
        .unwrap();
        assert_eq!(
            blobs,
            1 + 16 * 2,
            "_index, then each container's two Oodle files"
        );
        let data = data_with(one_blob_under_many_keys(&keys, &content));
        let mut called = false;
        let r = each_oodle_blob(data.path(), |_, _, _| {
            called = true;
            Ok::<(), Error>(())
        });
        assert!(
            matches!(r, Err(Error::BadArchive(ref m))
                if m.contains("more than 16 containers") && m.contains(&"50".repeat(20))),
            "{r:?}"
        );
        assert!(!called);
    }
}
