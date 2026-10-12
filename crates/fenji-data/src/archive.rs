//! The data archives' structure without decompression (M1 spec §5): each `data0NN` part's chain
//! of index blocks and their records, and the blob headers that records point at. The layout is
//! the engine's own reader's, FUN_1400bf780 and FUN_1400bff10 (docs/re/m1-archives.md).
use std::collections::HashSet;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

use crate::{BlobHeader, Codec, Error, FIXED_LEN, Result};

/// A part's first four bytes.
pub const PART_MAGIC: &[u8; 4] = b"ZFS\0";
/// An index block's first four bytes.
pub const INDEX_MAGIC: &[u8; 4] = b"[IX]";
/// A container's first four bytes: a stored file that holds many others (FUN_1400beb00).
pub const CONTAINER_MAGIC: &[u8; 4] = b"[SP]";
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
    use crate::test_util::{lz4_blob, stored_blob, zfs_part};

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
}
