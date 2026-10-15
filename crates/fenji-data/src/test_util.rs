//! Synthetic inputs for tests, built from our own bytes: small PE32+ images, LZ4 and stored blobs,
//! an embedded file system and data archive parts laid out like the client's. Never game bytes.
use crate::{blob::FIXED_LEN, exe_fs::PIECE_LEN};

/// The image base of every synthetic image (the client's own is the same).
pub const IMAGE_BASE: u64 = 0x1_4000_0000;
/// RVA of `.rdata` in [`exe_with_files`] images.
pub const RDATA_RVA: u32 = 0x1000;
/// RVA of `.data` in [`exe_with_files`] images.
pub const DATA_RVA: u32 = 0x10_0000;

/// A PE32+ image whose sections are (name, RVA, file bytes), in order.
pub fn pe_image(sections: &[(&str, u32, Vec<u8>)]) -> Vec<u8> {
    const OPT_SIZE: usize = 0xF0;
    let headers = 0x40 + 4 + 20 + OPT_SIZE + 40 * sections.len();
    let mut raw_offset = headers.next_multiple_of(0x200);
    let mut out = vec![0u8; raw_offset];
    out[0..2].copy_from_slice(b"MZ");
    out[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    out[0x40..0x44].copy_from_slice(b"PE\0\0");
    let coff = 0x44;
    out[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
    out[coff + 2..coff + 4].copy_from_slice(&(sections.len() as u16).to_le_bytes());
    out[coff + 16..coff + 18].copy_from_slice(&(OPT_SIZE as u16).to_le_bytes());
    let opt = coff + 20;
    out[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
    out[opt + 24..opt + 32].copy_from_slice(&IMAGE_BASE.to_le_bytes());
    let mut entry = opt + OPT_SIZE;
    let mut data = Vec::new();
    for (name, rva, bytes) in sections {
        let mut n = [0u8; 8];
        n[..name.len()].copy_from_slice(name.as_bytes());
        out[entry..entry + 8].copy_from_slice(&n);
        out[entry + 8..entry + 12].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        out[entry + 12..entry + 16].copy_from_slice(&rva.to_le_bytes());
        out[entry + 16..entry + 20].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        out[entry + 20..entry + 24].copy_from_slice(&(raw_offset as u32).to_le_bytes());
        entry += 40;
        let padded = bytes.len().next_multiple_of(0x200);
        data.extend_from_slice(bytes);
        data.resize(data.len() + (padded - bytes.len()), 0);
        raw_offset += padded;
    }
    out.extend_from_slice(&data);
    out
}

/// A one-block LZ4 blob, the way the engine stores a small file.
pub fn lz4_blob(content: &[u8], timestamp: u64) -> Vec<u8> {
    let comp = lz4_flex::block::compress(content);
    let mut b = Vec::with_capacity(FIXED_LEN + 4 + comp.len());
    b.extend_from_slice(&(content.len() as u64).to_le_bytes());
    b.extend_from_slice(&timestamp.to_le_bytes());
    b.extend_from_slice(&((FIXED_LEN + 4 + comp.len()) as u64).to_le_bytes());
    b.extend_from_slice(&(content.len() as u32).to_le_bytes()); // block size: the whole file
    b.extend_from_slice(&1u16.to_le_bytes()); // codec 1: LZ4
    b.extend_from_slice(&0u16.to_le_bytes()); // check: meaning unknown
    b.extend_from_slice(&(comp.len() as u32).to_le_bytes());
    b.extend_from_slice(&comp);
    b
}

/// An LZ4 blob (codec 1) of already compressed blocks, `block_size` raw bytes each but the last.
pub fn lz4_blob_blocks(blocks: &[Vec<u8>], raw_size: u64, block_size: u32) -> Vec<u8> {
    blob_of_blocks(blocks, raw_size, block_size, 1)
}

/// A blob of `codec` whose blocks are already encoded, `block_size` raw bytes each but the last.
fn blob_of_blocks(blocks: &[Vec<u8>], raw_size: u64, block_size: u32, codec: u16) -> Vec<u8> {
    let stored = FIXED_LEN + 4 * blocks.len() + blocks.iter().map(Vec::len).sum::<usize>();
    let mut b = Vec::new();
    b.extend_from_slice(&raw_size.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&(stored as u64).to_le_bytes());
    b.extend_from_slice(&block_size.to_le_bytes());
    b.extend_from_slice(&codec.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    for c in blocks {
        b.extend_from_slice(&(c.len() as u32).to_le_bytes());
    }
    for c in blocks {
        b.extend_from_slice(c);
    }
    b
}

/// Output bytes per Oodle stream header: `fenji_oodle::QUANTUM`, here so that these streams can be
/// built without the `oodle` feature, as tools/oodle-helper's tests do.
const OODLE_QUANTUM: usize = 0x4_0000;

/// An Oodle stream (Kraken, decoder 6) of `len` copies of `byte`: one memset quantum per 256 KiB,
/// each behind its stream header. Six bytes for each 256 KiB of output.
pub fn oodle_memset_stream(len: usize, byte: u8) -> Vec<u8> {
    [0x8C, 0x06, 0x07, 0xFF, 0xFF, byte].repeat(len.div_ceil(OODLE_QUANTUM))
}

/// An Oodle stream (Kraken) whose quanta hold `content` as it is, each 256 KiB behind a stream
/// header that says so.
pub fn oodle_uncompressed_stream(content: &[u8]) -> Vec<u8> {
    let quanta = content.chunks(OODLE_QUANTUM);
    let mut s = Vec::with_capacity(content.len() + 2 * quanta.len());
    for quantum in quanta {
        s.extend_from_slice(&[0xCC, 0x06]);
        s.extend_from_slice(quantum);
    }
    s
}

/// One Oodle block as the archives hold it (docs/re/m1-archives.md 3.4): its raw length as a `u32`,
/// then its stream.
pub fn oodle_block(raw_len: usize, stream: &[u8]) -> Vec<u8> {
    let mut b = (raw_len as u32).to_le_bytes().to_vec();
    b.extend_from_slice(stream);
    b
}

/// An Oodle blob (codec 4) of `content`, in blocks of `block_size` raw bytes, each an uncompressed
/// stream behind its raw length: it decodes to `content`.
pub fn oodle_blob(content: &[u8], block_size: u32) -> Vec<u8> {
    let blocks: Vec<Vec<u8>> = content
        .chunks(block_size as usize)
        .map(|c| oodle_block(c.len(), &oodle_uncompressed_stream(c)))
        .collect();
    blob_of_blocks(&blocks, content.len() as u64, block_size, 4)
}

/// An Oodle blob (codec 4) of `raw_size` copies of `byte`, in memset blocks of `block_size` raw
/// bytes.
pub fn oodle_memset_blob(raw_size: usize, block_size: u32, byte: u8) -> Vec<u8> {
    let mut blocks = Vec::new();
    let mut left = raw_size;
    while left > 0 {
        let n = left.min(block_size as usize);
        blocks.push(oodle_block(n, &oodle_memset_stream(n, byte)));
        left -= n;
    }
    blob_of_blocks(&blocks, raw_size as u64, block_size, 4)
}

/// A stored blob (codec 0): the 32-byte header, then `content`, with no block list. The block size
/// field is min(size, 512 KiB), as in most of the client's stored blobs.
pub fn stored_blob(content: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(FIXED_LEN + content.len());
    b.extend_from_slice(&(content.len() as u64).to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&((FIXED_LEN + content.len()) as u64).to_le_bytes());
    b.extend_from_slice(&(content.len().min(0x8_0000) as u32).to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes()); // codec 0: stored
    b.extend_from_slice(&0u16.to_le_bytes()); // check: meaning unknown
    b.extend_from_slice(content);
    b
}

/// One record of a synthetic part for [`zfs_part`]: its status byte (1 in use, 0xff deleted), its
/// key, and the blob it points at.
pub type PartRecord = (u8, [u8; 20], Vec<u8>);

/// A data archive part laid out like the client's (docs/re/m1-archives.md): "ZFS\0", then one
/// index block per entry of `blocks`, each followed by its records' blobs and chained to the next.
/// Records get their blob's offset and size and a correct CRC.
pub fn zfs_part(blocks: &[Vec<PartRecord>]) -> Vec<u8> {
    use crate::archive::{INDEX_BLOCK_LEN, INDEX_MAGIC, PART_MAGIC, RECORD_LEN, crc16_arc};
    let mut p = PART_MAGIC.to_vec();
    for (n, records) in blocks.iter().enumerate() {
        let at = p.len();
        p.resize(at + INDEX_BLOCK_LEN, 0);
        p[at..at + 4].copy_from_slice(INDEX_MAGIC);
        for (slot, (status, key, blob)) in records.iter().enumerate() {
            let offset = p.len() as u32;
            p.extend_from_slice(blob);
            let r = at + 8 + slot * RECORD_LEN;
            p[r..r + 20].copy_from_slice(key);
            p[r + 20..r + 24].copy_from_slice(&offset.to_le_bytes());
            p[r + 24..r + 28].copy_from_slice(&(blob.len() as u32).to_le_bytes());
            let crc = crc16_arc(&p[r..r + 28]);
            p[r + 28..r + 30].copy_from_slice(&crc.to_le_bytes());
            p[r + 31] = *status;
        }
        if n + 1 < blocks.len() {
            let next = p.len() as u32;
            p[at + 4..at + 8].copy_from_slice(&next.to_le_bytes());
        }
    }
    p
}

/// One file of a synthetic container for [`container`]: its key, its path in the listing, and its
/// blob.
pub type ContainerFile<'a> = ([u8; 20], &'a str, Vec<u8>);

/// A container's content laid out like the client's (docs/re/m1-archives.md 4.3): the 18-byte
/// header, the listing of `<key>\t<path>` lines, one record per file in the part layout with its
/// offset counted from the content's first byte, then the files' blobs. A part stores it as a
/// [`stored_blob`].
pub fn container(files: &[ContainerFile<'_>]) -> Vec<u8> {
    use crate::archive::{
        CONTAINER_HEADER_LEN, CONTAINER_MAGIC, RECORD_LEN, STATUS_USED, crc16_arc,
    };
    let listing: String = files
        .iter()
        .map(|(key, path, _)| format!("{}\t{path}\n", crate::hex(key)))
        .collect();
    let records_at = CONTAINER_HEADER_LEN + listing.len();
    let mut c = CONTAINER_MAGIC.to_vec();
    for field in [files.len(), listing.len(), records_at] {
        c.extend_from_slice(&(field as u32).to_le_bytes());
    }
    let crc = crc16_arc(&c[4..16]);
    c.extend_from_slice(&crc.to_le_bytes());
    c.extend_from_slice(listing.as_bytes());
    let mut blob_at = records_at + RECORD_LEN * files.len();
    for (key, _, blob) in files {
        let mut r = [0u8; RECORD_LEN];
        r[..20].copy_from_slice(key);
        r[20..24].copy_from_slice(&(blob_at as u32).to_le_bytes());
        r[24..28].copy_from_slice(&(blob.len() as u32).to_le_bytes());
        let crc = crc16_arc(&r[..28]);
        r[28..30].copy_from_slice(&crc.to_le_bytes());
        r[31] = STATUS_USED;
        c.extend_from_slice(&r);
        blob_at += blob.len();
    }
    for (_, _, blob) in files {
        c.extend_from_slice(blob);
    }
    c
}

/// Appends NUL-terminated `text` to `.rdata` bytes, 8-aligned, and returns its VA.
fn rdata_string(rdata: &mut Vec<u8>, text: &str) -> u64 {
    let at = rdata.len();
    rdata.extend_from_slice(text.as_bytes());
    rdata.push(0);
    rdata.resize(rdata.len().next_multiple_of(8), 0);
    IMAGE_BASE + u64::from(RDATA_RVA) + at as u64
}

/// One embedded directory for [`exe_with_dirs`]: its name and its files, each `(path, blob)`.
pub type Dir<'a> = (&'a str, Vec<(&'a str, Vec<u8>)>);

/// An image with an embedded file system laid out like the client's
/// (docs/re/m1-script-format.md §1). Each file is `("<dir>/<path>", blob)`, and the directories
/// are mounted under `data` in order of first appearance. See [`exe_with_dirs`] for the layout.
pub fn exe_with_files(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut dirs: Vec<Dir<'_>> = Vec::new();
    for (full, blob) in files {
        let (dir, path) = full.split_once('/').expect("test paths are <dir>/<path>");
        match dirs.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, list)) => list.push((path, blob.clone())),
            None => dirs.push((dir, vec![(path, blob.clone())])),
        }
    }
    exe_with_dirs(&dirs)
}

/// An image with an embedded file system laid out like the client's, with the directories given
/// in mount order, so that one can be empty. Each directory is `(name, [(path, blob)])`.
///
/// `.rdata` holds the names and the blobs. Each blob is cut into pieces of [`PIECE_LEN`] bytes
/// with other bytes between them, as the client scatters them. `.data` holds some noise (40
/// bytes), then each directory's entry list (a file's continuation entries point at an empty
/// path and have size 0, as in the client) with its NULL entry, then the mount table and its
/// NULL entry.
pub fn exe_with_dirs(dirs: &[Dir<'_>]) -> Vec<u8> {
    let mut rdata = Vec::new();
    let empty = rdata_string(&mut rdata, "");
    let mount = rdata_string(&mut rdata, "data");
    let mut data = vec![0xAB; 40]; // noise: neither a list nor a mount table
    let mut table = Vec::new();
    for (dir, files) in dirs {
        let first = IMAGE_BASE + u64::from(DATA_RVA) + data.len() as u64;
        for (path, blob) in files {
            assert!(!blob.is_empty(), "a file needs at least one piece");
            let path_va = rdata_string(&mut rdata, path);
            let mut pieces = Vec::new();
            for piece in blob.chunks(PIECE_LEN) {
                pieces.push(IMAGE_BASE + u64::from(RDATA_RVA) + rdata.len() as u64);
                rdata.extend_from_slice(piece);
                rdata.extend_from_slice(&[0xEE; 24]); // something else lies between pieces
                rdata.resize(rdata.len().next_multiple_of(8), 0);
            }
            for (i, piece_va) in pieces.iter().enumerate() {
                let (path_ptr, size) = if i == 0 {
                    (path_va, blob.len() as u32)
                } else {
                    (empty, 0)
                };
                data.extend_from_slice(&path_ptr.to_le_bytes());
                data.extend_from_slice(&size.to_le_bytes());
                data.extend_from_slice(&u32::from(i + 1 < pieces.len()).to_le_bytes());
                data.extend_from_slice(&piece_va.to_le_bytes());
            }
        }
        data.extend_from_slice(&[0; 24]); // the list's NULL path
        table.extend_from_slice(&mount.to_le_bytes());
        table.extend_from_slice(&rdata_string(&mut rdata, dir).to_le_bytes());
        table.extend_from_slice(&first.to_le_bytes());
    }
    data.extend_from_slice(&table);
    data.extend_from_slice(&[0; 24]); // the mount table's NULL mount name
    assert!(
        rdata.len() < (DATA_RVA - RDATA_RVA) as usize,
        "test files too large for the layout"
    );
    pe_image(&[
        (".text", 0x800, vec![0xC3; 16]),
        (".rdata", RDATA_RVA, rdata),
        (".data", DATA_RVA, data),
    ])
}
