//! The file system embedded in gujianol.exe: scripts, UI XML and configs packed as blobs in
//! `.rdata` and listed by a mount table in `.data` (docs/re/m1-script-format.md §1).
//!
//! The mount table holds 24-byte entries (pointers to a mount name, a directory name and the
//! directory's first file entry) and ends at a NULL mount name. A directory's file entries are
//! 24 bytes too (a path pointer, a u32 stored size, a u32 continuation flag and a pointer to a
//! piece of the stored blob) and end at a NULL path. A file whose flag is set continues in the
//! following entries; its pieces, [`PIECE_LEN`] bytes each except the last, are concatenated.
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::{BlobHeader, Decompressor, Error, PeImage, Result, Section, blob};

/// Bytes in each piece of a multi-piece file except the last (the engine's walker passes 0x3FFC).
pub const PIECE_LEN: usize = 0x3FFC;
/// Size of a mount-table entry and of a file entry.
const ENTRY_LEN: u64 = 24;
/// The longest path the scanner reads.
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
    /// The path inside the directory, such as `addons/Login/Login.luac`. A path that can't be
    /// read is named `?unreadable-path-at-<address>`; bytes that aren't UTF-8 become U+FFFD.
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
    /// Finds the mount table in `.data` and walks every directory it lists.
    ///
    /// A candidate table is a run of well-formed mount entries (two `.rdata` names and a list in
    /// `.data`) ended by a NULL mount name. The first candidate in which some file has a blob
    /// header that agrees with its entry is the table. Its lists must each end at a NULL path inside
    /// `.data` and share no entries with each other or with the table; if they don't, the scan
    /// fails rather than guessing. A problem with one file (an unreadable path, a bad header, a
    /// damaged block, a piece outside the image) affects only that file. The scan's work is bounded
    /// by the size of `.data`.
    pub fn scan(image: &'a PeImage) -> Result<Self> {
        let data = image.section(".data").ok_or(Error::NoRegistry)?;
        let end = data.va + image.section_data(data).len() as u64;
        let mut scanner = Scanner::new(image, data);
        let mut known_bad = HashSet::new();
        let mut va = data.va;
        while va + ENTRY_LEN <= end {
            if !known_bad.contains(&va)
                && let Some(run) = mount_run(image, data, va)
            {
                if scanner.signature(&run)? {
                    let entries = scanner.build(va, &run)?;
                    return Ok(Self { image, entries });
                }
                // A table whose lists hold no sound file has no sound sub-table either.
                known_bad.extend((1..run.len() as u64).map(|i| va + i * ENTRY_LEN));
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

/// One mount-table entry.
struct Mount {
    mount: String,
    dir: String,
    list: u64,
}

/// The well-formed mount entries from `va` to a NULL mount name, if there is at least one.
fn mount_run(image: &PeImage, data: &Section, va: u64) -> Option<Vec<Mount>> {
    let mut run = Vec::new();
    let mut at = va;
    loop {
        let raw = in_section(image, data, at)?;
        let word =
            |i: usize| u64::from_le_bytes(raw[8 * i..8 * i + 8].try_into().expect("8 bytes"));
        if word(0) == 0 {
            break;
        }
        let mount = name(image, word(0))?;
        let dir = name(image, word(1))?;
        if image.section_of(word(2))? != data {
            return None;
        }
        run.push(Mount {
            mount,
            dir,
            list: word(2),
        });
        at += ENTRY_LEN;
    }
    (!run.is_empty()).then_some(run)
}

/// One file as its entries list it.
struct RawFile {
    path: String,
    size: u32,
    pieces: Vec<u64>,
}

/// One list, walked.
struct Walk {
    files: Vec<RawFile>,
    /// Every entry the walk read, its NULL entry included.
    entries: Vec<u64>,
    /// Whether the list ended at a NULL path inside `.data`.
    ended: bool,
    /// Whether some file has a blob header that agrees with its entry.
    sound: bool,
}

/// Walks lists once each, within a budget of entries proportional to `.data`.
struct Scanner<'i> {
    image: &'i PeImage,
    data: &'i Section,
    walks: HashMap<u64, Rc<Walk>>,
    budget: u64,
}

impl<'i> Scanner<'i> {
    fn new(image: &'i PeImage, data: &'i Section) -> Self {
        let entries = image.section_data(data).len() as u64 / ENTRY_LEN;
        Self {
            image,
            data,
            walks: HashMap::new(),
            budget: 2 * entries + 1024,
        }
    }

    /// Whether some file in `run`'s lists has a sound header.
    fn signature(&mut self, run: &[Mount]) -> Result<bool> {
        for m in run {
            if self.walk(m.list)?.sound {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The files of the table at `va`, walked strictly.
    fn build(&mut self, va: u64, run: &[Mount]) -> Result<Vec<ExeEntry>> {
        let table_end = va + ENTRY_LEN * (run.len() as u64 + 1);
        let mut used: Vec<u64> = (va..table_end).step_by(ENTRY_LEN as usize).collect();
        let mut entries = Vec::new();
        for m in run {
            let walk = self.walk(m.list)?;
            if !walk.ended {
                return Err(Error::BadEntry {
                    va: m.list,
                    reason: format!("the list of {} runs off the end of .data", m.dir),
                });
            }
            used.extend(&walk.entries);
            entries.extend(walk.files.iter().map(|f| ExeEntry {
                mount: m.mount.clone(),
                dir: m.dir.clone(),
                path: f.path.clone(),
                stored_size: u64::from(f.size),
                pieces: f.pieces.clone(),
            }));
        }
        used.sort_unstable();
        if let Some(w) = used.windows(2).find(|w| w[1] - w[0] < ENTRY_LEN) {
            return Err(Error::BadEntry {
                va: w[1],
                reason: "two lists, or a list and the mount table, share entries".into(),
            });
        }
        Ok(entries)
    }

    /// The list at `first`, walked once. Fails when the scan's budget is spent.
    fn walk(&mut self, first: u64) -> Result<Rc<Walk>> {
        if let Some(w) = self.walks.get(&first) {
            return Ok(Rc::clone(w));
        }
        let mut w = Walk {
            files: Vec::new(),
            entries: Vec::new(),
            ended: false,
            sound: false,
        };
        let mut at = first;
        while let Some(e) = self.entry(at)? {
            w.entries.push(at);
            if e.path == 0 {
                w.ended = true;
                break;
            }
            let mut file = RawFile {
                path: path_of(self.image, e.path),
                size: e.size,
                pieces: vec![e.piece],
            };
            let mut more = e.more;
            while more != 0 {
                // A continuation entry has size 0 and a piece; anything else starts a new file, so
                // a stray flag can't swallow a file or a list's end.
                match self.entry(at + ENTRY_LEN)? {
                    Some(next) if next.size == 0 && next.piece != 0 => {
                        at += ENTRY_LEN;
                        w.entries.push(at);
                        file.pieces.push(next.piece);
                        more = next.more;
                    }
                    _ => break,
                }
            }
            w.sound |= sound(self.image, &file);
            w.files.push(file);
            at += ENTRY_LEN;
        }
        let w = Rc::new(w);
        self.walks.insert(first, Rc::clone(&w));
        Ok(w)
    }

    fn entry(&mut self, va: u64) -> Result<Option<FileEntry>> {
        if self.budget == 0 {
            return Err(Error::NoRegistry); // the work bound: no table within it
        }
        self.budget -= 1;
        Ok(FileEntry::read(self.image, self.data, va))
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
    if image.section_of(va)? != section {
        return None;
    }
    image.read(va, ENTRY_LEN as usize)
}

/// The non-empty string at `va` in `.rdata`, without control characters, at most `max` bytes.
fn rdata_bytes(image: &PeImage, va: u64, max: usize) -> Option<&[u8]> {
    if image.section_of(va)?.name != ".rdata" {
        return None;
    }
    let s = image.c_str(va, max)?;
    (!s.is_empty() && !s.iter().any(|&b| b < 0x20 || b == 0x7F)).then_some(s)
}

/// A mount or directory name: printable UTF-8 in `.rdata`.
fn name(image: &PeImage, va: u64) -> Option<String> {
    let s = rdata_bytes(image, va, MAX_NAME)?;
    std::str::from_utf8(s).ok().map(str::to_owned)
}

/// A file's path, as well as it can be read.
fn path_of(image: &PeImage, va: u64) -> String {
    rdata_bytes(image, va, MAX_PATH).map_or_else(
        || format!("?unreadable-path-at-{va:#x}"),
        |s| String::from_utf8_lossy(s).into_owned(),
    )
}

/// Whether the file's first piece starts with a blob header that agrees with its entry.
fn sound(image: &PeImage, f: &RawFile) -> bool {
    let len = (f.size as usize).min(PIECE_LEN);
    f.pieces
        .first()
        .and_then(|&p| image.read(p, len))
        .and_then(|b| BlobHeader::parse(b, 0).ok())
        .is_some_and(|h| h.stored_size == u64::from(f.size))
}

/// The file's stored blob, its pieces joined.
fn join(image: &PeImage, e: &ExeEntry) -> Result<Vec<u8>> {
    let bad = |reason: String| Error::BadEntry {
        va: e.header_va(),
        reason,
    };
    // Its pieces could repeat one region, so cap the size by the image before reserving it.
    if e.stored_size > image.file_len() as u64 {
        return Err(bad(format!(
            "it claims {} stored bytes, more than the image's {}",
            e.stored_size,
            image.file_len()
        )));
    }
    let size = e.stored_size as usize;
    let needed = size.div_ceil(PIECE_LEN).max(1);
    if e.pieces.len() != needed {
        return Err(bad(format!(
            "{} pieces for {size} stored bytes, where {needed} are needed",
            e.pieces.len()
        )));
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
    use crate::test_util::{
        DATA_RVA, IMAGE_BASE, RDATA_RVA, exe_with_dirs, exe_with_files, lz4_blob, pe_image,
    };
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
    fn a_list_that_runs_into_the_mount_table_is_an_error() {
        // One file entry, then the mount table with no NULL entry between them: walking the list
        // reads the table's entries, so the table found is broken, and the scan says so rather
        // than guessing.
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
        assert!(matches!(ExeFs::scan(&img), Err(Error::BadEntry { .. })));
    }

    /// Two directories of two files each (core: a, b; scripts: c, d), and the image position of
    /// each file's entry, in that order.
    fn two_dirs() -> (Vec<u8>, Vec<usize>) {
        let pe = exe_with_files(&[
            ("core/a.luac", lz4_blob(b"a", 0)),
            ("core/b.luac", lz4_blob(b"b", 0)),
            ("scripts/c.luac", lz4_blob(b"c", 0)),
            ("scripts/d.luac", lz4_blob(b"d", 0)),
        ]);
        let pieces: Vec<u64> = {
            let img = PeImage::parse(pe.clone()).unwrap();
            ExeFs::scan(&img)
                .unwrap()
                .entries()
                .iter()
                .map(|e| e.pieces[0])
                .collect()
        };
        let at = pieces.iter().map(|&p| entry_at(&pe, p)).collect();
        (pe, at)
    }

    fn read_all(img: &PeImage) -> Vec<(String, Result<Vec<u8>>)> {
        let fs = ExeFs::scan(img).unwrap();
        fs.entries()
            .iter()
            .map(|e| (e.engine_path(), fs.read(e, &BuiltinCodecs)))
            .collect()
    }

    #[test]
    fn an_empty_directory_keeps_the_others() {
        let img = PeImage::parse(exe_with_dirs(&[
            ("core", vec![("a.luac", lz4_blob(b"a", 0))]),
            ("empty", vec![]),
            ("scripts", vec![("b.luac", lz4_blob(b"b", 0))]),
        ]))
        .unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        let got: Vec<String> = fs.entries().iter().map(ExeEntry::engine_path).collect();
        assert_eq!(got, ["data/core/a.luac", "data/scripts/b.luac"]);
    }

    #[test]
    fn a_bad_first_file_is_an_error_for_that_file_only() {
        // The first file of each directory claims one byte more than its blob holds.
        let (mut pe, at) = two_dirs();
        for i in [at[0], at[2]] {
            let size = u32::from_le_bytes(pe[i + 8..i + 12].try_into().unwrap()) + 1;
            pe[i + 8..i + 12].copy_from_slice(&size.to_le_bytes());
        }
        let got = read_all(&PeImage::parse(pe).unwrap());
        assert_eq!(got.len(), 4);
        assert!(got[0].1.is_err() && got[2].1.is_err());
        assert_eq!(got[1].1.as_deref().unwrap(), b"b");
        assert_eq!(got[3].1.as_deref().unwrap(), b"d");
    }

    #[test]
    fn odd_paths_are_still_listed() {
        // b's path pointer leads nowhere, and c's path holds a byte that isn't UTF-8.
        let (mut pe, at) = two_dirs();
        pe[at[1]..at[1] + 8].copy_from_slice(&(IMAGE_BASE + 0x50_0000).to_le_bytes());
        let c = pe.windows(7).position(|w| w == b"c.luac\0").unwrap();
        pe[c] = 0xD6;
        let img = PeImage::parse(pe).unwrap();
        let fs = ExeFs::scan(&img).unwrap();
        let paths: Vec<&str> = fs.entries().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths.len(), 4, "{paths:?}");
        assert!(paths[1].contains("unreadable"), "{paths:?}");
        assert_eq!(paths[2], "\u{FFFD}.luac");
        assert_eq!(fs.read(&fs.entries()[1], &BuiltinCodecs).unwrap(), b"b");
    }

    #[test]
    fn a_stray_continuation_flag_drops_no_file() {
        // a says it continues, but the entry after it is b, a file of its own.
        let (mut pe, at) = two_dirs();
        pe[at[0] + 12..at[0] + 16].copy_from_slice(&1u32.to_le_bytes());
        let got = read_all(&PeImage::parse(pe).unwrap());
        let paths: Vec<&str> = got.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "data/core/a.luac",
                "data/core/b.luac",
                "data/scripts/c.luac",
                "data/scripts/d.luac"
            ]
        );
        assert!(got.iter().all(|(_, r)| r.is_ok()));
    }

    #[test]
    fn a_flag_on_a_last_file_does_not_run_into_the_next_directory() {
        // core's last file says it continues, into its list's NULL entry and beyond.
        let (mut pe, at) = two_dirs();
        pe[at[1] + 12..at[1] + 16].copy_from_slice(&1u32.to_le_bytes());
        let got = read_all(&PeImage::parse(pe).unwrap());
        let paths: Vec<&str> = got.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "data/core/a.luac",
                "data/core/b.luac",
                "data/scripts/c.luac",
                "data/scripts/d.luac"
            ]
        );
    }

    #[test]
    fn directories_sharing_entries_are_refused() {
        // The second mount entry names the first directory's list.
        let (mut pe, _) = two_dirs();
        let list1 = IMAGE_BASE + u64::from(DATA_RVA) + 40; // after exe_with_dirs' 40 bytes of noise
        let list2 = list1 + 3 * 24; // core's two entries and its NULL entry
        let at = pe
            .windows(8)
            .position(|w| w == list2.to_le_bytes())
            .unwrap();
        pe[at..at + 8].copy_from_slice(&list1.to_le_bytes());
        let img = PeImage::parse(pe).unwrap();
        assert!(matches!(ExeFs::scan(&img), Err(Error::BadEntry { .. })));
    }

    /// `.rdata` with the names `data`, `scripts` and `f.luac`, a small sound blob, and a 16 KiB
    /// region of zeros: (bytes, mount, dir, path, blob VA, blob length, zeros VA).
    fn rdata_parts() -> (Vec<u8>, [u64; 6]) {
        let base = IMAGE_BASE + u64::from(RDATA_RVA);
        let mut r = b"data\0\0\0\0scripts\0f.luac\0\0".to_vec();
        let blob = lz4_blob(b"ok", 0);
        let blob_at = r.len();
        r.extend_from_slice(&blob);
        r.resize(r.len().next_multiple_of(8), 0);
        let zeros_at = r.len();
        r.resize(zeros_at + PIECE_LEN, 0);
        let vas = [
            base,
            base + 8,
            base + 16,
            base + blob_at as u64,
            blob.len() as u64,
            base + zeros_at as u64,
        ];
        (r, vas)
    }

    fn file_entry(data: &mut Vec<u8>, path: u64, size: u32, more: bool, piece: u64) {
        data.extend_from_slice(&path.to_le_bytes());
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&u32::from(more).to_le_bytes());
        data.extend_from_slice(&piece.to_le_bytes());
    }

    #[test]
    fn a_file_larger_than_the_image_is_refused_before_allocating() {
        // A sound file, then one that claims more bytes than the whole image in pieces that all
        // repeat one 16 KiB region.
        let (rdata, [mount, dir, path, blob, blob_len, zeros]) = rdata_parts();
        let mut data = Vec::new();
        file_entry(&mut data, path, blob_len as u32, false, blob);
        let pieces = 8;
        file_entry(&mut data, path, (pieces * PIECE_LEN) as u32, true, zeros);
        for i in 1..pieces {
            file_entry(&mut data, path, 0, i + 1 < pieces, zeros);
        }
        data.extend_from_slice(&[0; 24]);
        let list = IMAGE_BASE + u64::from(DATA_RVA);
        for w in [mount, dir, list] {
            data.extend_from_slice(&w.to_le_bytes());
        }
        data.extend_from_slice(&[0; 24]);
        let img = PeImage::parse(pe_image(&[
            (".rdata", RDATA_RVA, rdata),
            (".data", DATA_RVA, data),
        ]))
        .unwrap();
        assert!(img.file_len() < pieces * PIECE_LEN);
        let fs = ExeFs::scan(&img).unwrap();
        let err = fs.read(&fs.entries()[1], &BuiltinCodecs).unwrap_err();
        assert!(
            matches!(&err, Error::BadEntry { reason, .. } if reason.contains("image")),
            "{err}"
        );
    }

    #[test]
    fn many_candidate_tables_are_given_up_quickly() {
        // A long list of files without a sound header, and as many mount entries, each naming a
        // different point in it: checking every candidate in full would take quadratic time.
        let (rdata, [mount, dir, path, _, _, zeros]) = rdata_parts();
        let n = 8000u64;
        let mut data = Vec::new();
        for _ in 0..n {
            file_entry(&mut data, path, 64, false, zeros);
        }
        data.extend_from_slice(&[0; 24]);
        let list = IMAGE_BASE + u64::from(DATA_RVA);
        for i in 0..n {
            for w in [mount, dir, list + 24 * i] {
                data.extend_from_slice(&w.to_le_bytes());
            }
        }
        data.extend_from_slice(&[0; 24]);
        let img = PeImage::parse(pe_image(&[
            (".rdata", RDATA_RVA, rdata),
            (".data", DATA_RVA, data),
        ]))
        .unwrap();
        let started = std::time::Instant::now();
        assert!(ExeFs::scan(&img).is_err());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }
}
