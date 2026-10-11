//! The file system embedded in gujianol.exe: scripts, UI XML and configs packed as blobs in
//! `.rdata` and listed by a mount table in `.data` (docs/re/m1-script-format.md §1).
//!
//! The mount table holds 24-byte entries (pointers to a mount name, a directory name and the
//! directory's first file entry) and ends at a NULL mount name. A directory's file entries are
//! 24 bytes too (a path pointer, a u32 stored size, a u32 continuation flag and a pointer to a
//! piece of the stored blob) and end at a NULL path. A file whose flag is set continues in the
//! following entries; its pieces, [`PIECE_LEN`] bytes each except the last, are concatenated.
use crate::{BlobHeader, Decompressor, Error, PeImage, Result, Section, blob};

/// Bytes in each piece of a multi-piece file except the last (the engine's walker passes 0x3FFC).
pub const PIECE_LEN: usize = 0x3FFC;
/// Size of a mount-table entry and of a file entry.
const ENTRY_LEN: usize = 24;
/// The longest path the scanner accepts.
const MAX_PATH: usize = 260;
/// The longest mount or directory name the scanner accepts.
const MAX_NAME: usize = 64;

/// One file of the embedded file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeEntry {
    /// The mount its directory hangs under: `data` for every directory in the pinned build.
    pub mount: String,
    /// The embedded directory: `c`, `locale`, `core`, `interface` or `scripts` in the pinned build.
    pub dir: String,
    /// The path inside the directory, such as `addons/Login/Login.luac`.
    pub path: String,
    /// Size of the stored blob, header included.
    pub stored_size: u64,
    /// Virtual address of each piece of the stored blob, in order.
    pub pieces: Vec<u64>,
}

impl ExeEntry {
    /// The path the engine opens the file by: `<mount>/<dir>/<path>`.
    pub fn engine_path(&self) -> String {
        format!("{}/{}/{}", self.mount, self.dir, self.path)
    }

    /// Virtual address of the blob header: the start of the first piece.
    pub fn header_va(&self) -> u64 {
        self.pieces.first().copied().unwrap_or_default()
    }
}

/// The embedded file system of one image.
#[derive(Debug)]
pub struct ExeFs<'a> {
    image: &'a PeImage,
    entries: Vec<ExeEntry>,
}

impl<'a> ExeFs<'a> {
    /// Finds the mount table in `.data` (8-byte aligned) and walks every directory it lists. A
    /// candidate table counts only if every list ends at a NULL path inside `.data` and the first
    /// file of each list has a blob header that agrees with its entry.
    pub fn scan(image: &'a PeImage) -> Result<Self> {
        let data = image.section(".data").ok_or(Error::NoRegistry)?;
        let end = data.va + image.section_data(data).len() as u64;
        let mut va = data.va;
        while va + ENTRY_LEN as u64 <= end {
            if let Some(entries) = mount_table(image, data, va) {
                return Ok(Self { image, entries });
            }
            va += 8;
        }
        Err(Error::NoRegistry)
    }

    /// Every file, in mount-table order and then list order.
    pub fn entries(&self) -> &[ExeEntry] {
        &self.entries
    }

    /// The file's blob header.
    pub fn header(&self, e: &ExeEntry) -> Result<BlobHeader> {
        header_of(&join(self.image, e)?, e)
    }

    /// The file's decoded content.
    pub fn read(&self, e: &ExeEntry, codecs: &dyn Decompressor) -> Result<Vec<u8>> {
        let stored = join(self.image, e)?;
        header_of(&stored, e)?;
        blob::decode(&stored, e.header_va(), codecs)
    }
}

/// The files of the mount table at `va`, if a sound one starts there.
fn mount_table(image: &PeImage, data: &Section, va: u64) -> Option<Vec<ExeEntry>> {
    let mut entries = Vec::new();
    let mut at = va;
    loop {
        let raw = in_section(image, data, at)?;
        let word =
            |i: usize| u64::from_le_bytes(raw[8 * i..8 * i + 8].try_into().expect("8 bytes"));
        if word(0) == 0 {
            break;
        }
        let mount = rdata_str(image, word(0), MAX_NAME)?;
        let dir = rdata_str(image, word(1), MAX_NAME)?;
        let files = walk_list(image, data, word(2), &mount, &dir)?;
        let first = files.first()?;
        header_of(&join(image, first).ok()?, first).ok()?;
        entries.extend(files);
        at += ENTRY_LEN as u64;
    }
    (at != va).then_some(entries)
}

/// The files of one directory's list, if it ends at a NULL path inside `.data`.
fn walk_list(
    image: &PeImage,
    data: &Section,
    first: u64,
    mount: &str,
    dir: &str,
) -> Option<Vec<ExeEntry>> {
    let mut files = Vec::new();
    let mut at = first;
    loop {
        let e = FileEntry::read(image, data, at)?;
        if e.path == 0 {
            return Some(files);
        }
        let path = rdata_str(image, e.path, MAX_PATH)?;
        let mut pieces = vec![e.piece];
        let mut more = e.more;
        while more != 0 {
            at += ENTRY_LEN as u64;
            let next = FileEntry::read(image, data, at)?;
            pieces.push(next.piece);
            more = next.more;
        }
        let (mount, dir) = (mount.to_owned(), dir.to_owned());
        files.push(ExeEntry {
            mount,
            dir,
            path,
            stored_size: u64::from(e.size),
            pieces,
        });
        at += ENTRY_LEN as u64;
    }
}

/// One 24-byte file entry as stored.
struct FileEntry {
    path: u64,
    size: u32,
    more: u32,
    piece: u64,
}

impl FileEntry {
    fn read(image: &PeImage, data: &Section, va: u64) -> Option<Self> {
        let raw = in_section(image, data, va)?;
        Some(Self {
            path: u64::from_le_bytes(raw[0..8].try_into().ok()?),
            size: u32::from_le_bytes(raw[8..12].try_into().ok()?),
            more: u32::from_le_bytes(raw[12..16].try_into().ok()?),
            piece: u64::from_le_bytes(raw[16..24].try_into().ok()?),
        })
    }
}

/// The 24 bytes at `va`, if they lie in `section`'s file data.
fn in_section<'i>(image: &'i PeImage, section: &Section, va: u64) -> Option<&'i [u8]> {
    if image.section_of(va)?.name != section.name {
        return None;
    }
    image.read(va, ENTRY_LEN)
}

/// The non-empty printable string at `va` in `.rdata`.
fn rdata_str(image: &PeImage, va: u64, max: usize) -> Option<String> {
    if image.section_of(va)?.name != ".rdata" {
        return None;
    }
    let s = image.c_str(va, max)?;
    if s.is_empty() || s.iter().any(|&b| b < 0x20 || b == 0x7F) {
        return None;
    }
    std::str::from_utf8(s).ok().map(str::to_owned)
}

/// The file's stored blob, its pieces joined.
fn join(image: &PeImage, e: &ExeEntry) -> Result<Vec<u8>> {
    let size = e.stored_size as usize; // from a u32 field
    let needed = size.div_ceil(PIECE_LEN).max(1);
    if e.pieces.len() != needed {
        let reason = format!(
            "{} pieces for {size} stored bytes, where {needed} are needed",
            e.pieces.len()
        );
        return Err(Error::BadEntry {
            va: e.header_va(),
            reason,
        });
    }
    let mut out = Vec::with_capacity(size);
    for &piece in &e.pieces {
        let len = (size - out.len()).min(PIECE_LEN);
        let bytes = image.read(piece, len).ok_or(Error::OutOfImage {
            va: piece,
            len: len as u64,
        })?;
        out.extend_from_slice(bytes);
    }
    Ok(out)
}

/// The blob header of `stored`, checked against the entry's size.
fn header_of(stored: &[u8], e: &ExeEntry) -> Result<BlobHeader> {
    let h = BlobHeader::parse(stored, e.header_va())?;
    if h.stored_size != e.stored_size {
        let reason = format!(
            "the blob header says {} stored bytes, the entry {}",
            h.stored_size, e.stored_size
        );
        return Err(Error::BadEntry {
            va: e.header_va(),
            reason,
        });
    }
    Ok(h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{DATA_RVA, IMAGE_BASE, RDATA_RVA, exe_with_files, lz4_blob, pe_image};
    use crate::{BuiltinCodecs, FIXED_LEN};

    /// Bytes LZ4 can't shrink, so a blob of them spans several pieces.
    fn noise(len: usize) -> Vec<u8> {
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            })
            .collect()
    }

    /// Byte position in `image` of the file entry whose first piece is at `piece_va`.
    fn entry_at(image: &[u8], piece_va: u64) -> usize {
        let needle = piece_va.to_le_bytes();
        let hits: Vec<usize> = image
            .windows(8)
            .enumerate()
            .filter(|(_, w)| *w == needle)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hits.len(), 1, "piece VA {piece_va:#x} must occur once");
        hits[0] - 16
    }

    /// Two small files in one directory, and the image position of the second one's entry.
    fn two_files() -> (Vec<u8>, usize) {
        let pe = exe_with_files(&[
            ("scripts/a.luac", lz4_blob(b"first", 0)),
            ("scripts/b.luac", lz4_blob(b"second", 0)),
        ]);
        let piece = {
            let img = PeImage::parse(pe.clone()).unwrap();
            ExeFs::scan(&img).unwrap().entries()[1].pieces[0]
        };
        let at = entry_at(&pe, piece);
        (pe, at)
    }

    #[test]
    fn scan_lists_every_directory_and_file_in_order() {
        let img = PeImage::parse(exe_with_files(&[
            ("scripts/combat/x.luac", lz4_blob(&b"hello ".repeat(30), 7)),
            ("interface/addons/Login/Login.xml", lz4_blob(b"<UI/>", 7)),
            ("scripts/combat/y.luac", lz4_blob(b"local y", 7)),
        ]))
        .unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        let got: Vec<String> = fs.entries().iter().map(ExeEntry::engine_path).collect();
        assert_eq!(
            got,
            [
                "data/scripts/combat/x.luac",
                "data/scripts/combat/y.luac",
                "data/interface/addons/Login/Login.xml"
            ]
        );
        let login = &fs.entries()[2];
        assert_eq!(
            (
                login.mount.as_str(),
                login.dir.as_str(),
                login.path.as_str()
            ),
            ("data", "interface", "addons/Login/Login.xml")
        );
        assert_eq!(
            fs.read(&fs.entries()[0], &BuiltinCodecs).unwrap(),
            b"hello ".repeat(30)
        );
        assert_eq!(fs.header(login).unwrap().raw_size, 5);
    }

    #[test]
    fn multi_piece_file_is_joined_from_scattered_pieces() {
        let content = noise(40_000);
        let blob = lz4_blob(&content, 9);
        assert!(blob.len() > 2 * PIECE_LEN, "the test needs three pieces");
        let img = PeImage::parse(exe_with_files(&[
            ("core/big.luac", blob.clone()),
            ("core/small.toc", lz4_blob(b"## x", 9)),
        ]))
        .unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        let big = &fs.entries()[0];
        assert_eq!((big.pieces.len(), big.stored_size), (3, blob.len() as u64));
        assert_eq!(fs.read(big, &BuiltinCodecs).unwrap(), content);
        // The continuation entries were not taken for files.
        assert_eq!(fs.entries().len(), 2);
        assert_eq!(fs.read(&fs.entries()[1], &BuiltinCodecs).unwrap(), b"## x");
    }

    #[test]
    fn a_corrupt_block_is_an_error_for_that_file_only() {
        // The header and the entry agree, but the compressed bytes are damaged.
        let mut blob = lz4_blob(&b"abcdefgh".repeat(100), 0);
        for b in &mut blob[FIXED_LEN + 4..] {
            *b ^= 0x5A;
        }
        let img = PeImage::parse(exe_with_files(&[
            ("scripts/ok.luac", lz4_blob(b"ok", 0)),
            ("scripts/bad.luac", blob),
        ]))
        .unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        assert!(fs.read(&fs.entries()[1], &BuiltinCodecs).is_err());
        assert_eq!(fs.read(&fs.entries()[0], &BuiltinCodecs).unwrap(), b"ok");
    }

    #[test]
    fn a_piece_outside_the_image_is_an_error_for_that_file_only() {
        let (mut pe, at) = two_files();
        pe[at + 16..at + 24].copy_from_slice(&(IMAGE_BASE + 0x50_0000).to_le_bytes()); // in no section
        let img = PeImage::parse(pe).unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        assert!(matches!(
            fs.read(&fs.entries()[1], &BuiltinCodecs),
            Err(Error::OutOfImage { .. })
        ));
        assert_eq!(fs.read(&fs.entries()[0], &BuiltinCodecs).unwrap(), b"first");
    }

    #[test]
    fn a_size_that_needs_more_pieces_is_an_error_for_that_file_only() {
        let (mut pe, at) = two_files();
        pe[at + 8..at + 12].copy_from_slice(&(PIECE_LEN as u32 + 1).to_le_bytes());
        let img = PeImage::parse(pe).unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        assert!(matches!(
            fs.read(&fs.entries()[1], &BuiltinCodecs),
            Err(Error::BadEntry { .. })
        ));
        assert!(matches!(
            fs.header(&fs.entries()[1]),
            Err(Error::BadEntry { .. })
        ));
        assert_eq!(fs.read(&fs.entries()[0], &BuiltinCodecs).unwrap(), b"first");
    }

    #[test]
    fn no_registry_is_an_error() {
        let img = PeImage::parse(pe_image(&[
            (".rdata", 0x1000, vec![0; 64]),
            (".data", 0x2000, vec![0; 64]),
        ]))
        .unwrap();
        assert!(matches!(ExeFs::scan(&img), Err(Error::NoRegistry)));
    }

    #[test]
    fn a_list_that_runs_off_data_is_not_a_registry() {
        // One file entry, then the mount table with no NULL entry between them: walking the list
        // reads the table as entries and runs off the end of .data.
        let blob = lz4_blob(b"x", 0);
        let rva = |off: usize| IMAGE_BASE + u64::from(RDATA_RVA) + off as u64;
        let mut rdata = b"data\0\0\0\0scripts\0a.luac\0\0".to_vec();
        let blob_at = rdata.len();
        rdata.extend_from_slice(&blob);
        let mut data = Vec::new();
        data.extend_from_slice(&rva(16).to_le_bytes());
        data.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&rva(blob_at).to_le_bytes());
        data.extend_from_slice(&rva(0).to_le_bytes());
        data.extend_from_slice(&rva(8).to_le_bytes());
        data.extend_from_slice(&(IMAGE_BASE + u64::from(DATA_RVA)).to_le_bytes());
        data.extend_from_slice(&[0; 24]);
        let img = PeImage::parse(pe_image(&[
            (".rdata", RDATA_RVA, rdata),
            (".data", DATA_RVA, data),
        ]))
        .unwrap();
        assert!(matches!(ExeFs::scan(&img), Err(Error::NoRegistry)));
    }
}
