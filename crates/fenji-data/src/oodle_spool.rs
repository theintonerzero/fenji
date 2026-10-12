//! Codec 4 (Oodle LZ) through a spool that `tools/oodle-helper` works on inside the sandbox (M2
//! spec §6; gate #39, route c). A missing block is written to `in/<name>`; the helper writes
//! `out/<name>`. The name binds the input's SHA-1 and the raw length, so an output can only be
//! used for the block it was made from, and only at the right length.
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};

use crate::blob::blocks;
use crate::{BuiltinCodecs, Codec, Decompressor, Error, Result};

/// A spool directory with `in/` and `out/`.
pub struct OodleSpool {
    dir: PathBuf,
}

/// Writes `path` through a temporary file, so a reader never sees a partial file. Whatever already
/// sits at the temporary path (a link, say) is replaced, never written through.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut f = fs::File::options()
        .write(true)
        .create_new(true)
        .open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    Ok(())
}

impl OodleSpool {
    /// Opens the spool at `dir`, creating `dir/in` and `dir/out`.
    pub fn new(dir: &Path) -> Result<Self> {
        fs::create_dir_all(dir.join("in"))?;
        fs::create_dir_all(dir.join("out"))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// `<SHA-1 of the input, lowercase hex>-<raw length>.bin`.
    pub fn name(input: &[u8], raw_len: usize) -> String {
        let digest = Sha1::digest(input);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("{hex}-{raw_len}.bin")
    }

    /// Queues every codec-4 block of the blob at `bytes` that has no output yet. Returns how many
    /// it newly queued.
    pub fn spool_blob(&self, bytes: &[u8], at: u64) -> Result<usize> {
        let (h, list) = blocks(bytes, at)?;
        if h.codec != Codec::Oodle {
            return Ok(0);
        }
        let mut queued = 0;
        for b in list {
            let name = Self::name(b.input, b.raw_len);
            let (inp, out) = (
                self.dir.join("in").join(&name),
                self.dir.join("out").join(&name),
            );
            if !out.exists() && !inp.exists() {
                write_atomic(&inp, b.input)?;
                queued += 1;
            }
        }
        Ok(queued)
    }
}

impl Decompressor for OodleSpool {
    fn decompress_block(
        &self,
        codec: Codec,
        input: &[u8],
        raw_len: usize,
        index: usize,
    ) -> Result<Vec<u8>> {
        if codec != Codec::Oodle {
            return BuiltinCodecs.decompress_block(codec, input, raw_len, index);
        }
        let name = Self::name(input, raw_len);
        let out = self.dir.join("out").join(&name);
        let bad = |problem: String| Error::OodleBadOutput {
            path: out.display().to_string(),
            problem,
        };
        // out/ is written from inside the sandbox: only a regular file there counts.
        let read = match fs::symlink_metadata(&out) {
            Ok(m) if !m.file_type().is_file() => return Err(bad("not a regular file".into())),
            Ok(_) => fs::read(&out),
            Err(e) => Err(e),
        };
        match read {
            Ok(bytes) if bytes.len() == raw_len => Ok(bytes),
            Ok(bytes) => Err(bad(format!(
                "{} bytes, the block decodes to {raw_len}",
                bytes.len()
            ))),
            Err(e) if e.kind() == ErrorKind::NotFound => {
                let inp = self.dir.join("in").join(&name);
                if !inp.exists() {
                    write_atomic(&inp, input)?;
                }
                Err(Error::OodlePending(name))
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spool() -> (tempfile::TempDir, OodleSpool) {
        let tmp = tempfile::tempdir().unwrap();
        let s = OodleSpool::new(tmp.path()).unwrap();
        (tmp, s)
    }

    #[test]
    fn name_binds_raw_len() {
        assert_ne!(OodleSpool::name(b"abc", 10), OodleSpool::name(b"abc", 11));
        assert_eq!(
            OodleSpool::name(b"abc", 10),
            "a9993e364706816aba3e25717850c26c9cd0d89d-10.bin"
        );
    }

    #[test]
    fn miss_spools_then_hit_reads_output() {
        let (tmp, s) = spool();
        let err = s
            .decompress_block(Codec::Oodle, b"packed", 4, 0)
            .unwrap_err();
        let Error::OodlePending(name) = err else {
            panic!("{err:?}")
        };
        assert_eq!(
            fs::read(tmp.path().join("in").join(&name)).unwrap(),
            b"packed"
        );
        fs::write(tmp.path().join("out").join(&name), b"raw!").unwrap();
        assert_eq!(
            s.decompress_block(Codec::Oodle, b"packed", 4, 0).unwrap(),
            b"raw!"
        );
    }

    #[test]
    fn short_output_is_not_accepted() {
        let (tmp, s) = spool();
        let name = OodleSpool::name(b"packed", 4);
        fs::write(tmp.path().join("out").join(&name), b"raw").unwrap();
        let err = s
            .decompress_block(Codec::Oodle, b"packed", 4, 0)
            .unwrap_err();
        let Error::OodleBadOutput { path, problem } = err else {
            panic!("{err:?}")
        };
        assert!(path.ends_with(&name), "{path}");
        assert!(problem.starts_with("3 bytes"), "{problem}");
    }

    #[cfg(unix)]
    #[test]
    fn a_planted_temp_link_is_replaced_not_followed() {
        // The sandboxed helper can't write in/, but should anything put a link where the spool
        // writes its temporary file, the link is replaced, never written through.
        let (tmp, s) = spool();
        let victim = tmp.path().join("victim");
        fs::write(&victim, b"keep").unwrap();
        let name = OodleSpool::name(b"packed", 4);
        let link = tmp.path().join("in").join(&name).with_extension("tmp");
        std::os::unix::fs::symlink(&victim, link).unwrap();
        let _ = s.decompress_block(Codec::Oodle, b"packed", 4, 0);
        assert_eq!(fs::read(&victim).unwrap(), b"keep");
        assert_eq!(
            fs::read(tmp.path().join("in").join(&name)).unwrap(),
            b"packed"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_output_is_refused() {
        // out/ is the sandbox's to write: only a regular file there counts as an output.
        let (tmp, s) = spool();
        let elsewhere = tmp.path().join("elsewhere");
        fs::write(&elsewhere, b"raw!").unwrap();
        let name = OodleSpool::name(b"packed", 4);
        std::os::unix::fs::symlink(&elsewhere, tmp.path().join("out").join(&name)).unwrap();
        assert!(s.decompress_block(Codec::Oodle, b"packed", 4, 0).is_err());
    }

    #[test]
    fn pending_input_is_written_once() {
        let (tmp, s) = spool();
        let name = OodleSpool::name(b"packed", 4);
        let _ = s.decompress_block(Codec::Oodle, b"packed", 4, 0);
        let p = tmp.path().join("in").join(&name);
        let before = fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = s.decompress_block(Codec::Oodle, b"packed", 4, 0);
        assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn spool_blob_queues_each_missing_block_once() {
        // A three-block blob relabelled as codec 4: its blocks' bytes stand in for Oodle data.
        let blocks: Vec<Vec<u8>> = [b"one".to_vec(), b"two".to_vec(), b"three".to_vec()].into();
        let mut blob = crate::test_util::lz4_blob_blocks(&blocks, 10, 4);
        blob[28] = 4;
        let (tmp, s) = spool();
        // The second block already has its output: only the other two are queued.
        fs::write(
            tmp.path().join("out").join(OodleSpool::name(b"two", 4)),
            b"2222",
        )
        .unwrap();
        assert_eq!(s.spool_blob(&blob, 0).unwrap(), 2);
        assert_eq!(s.spool_blob(&blob, 0).unwrap(), 0, "already queued");
        let input = |n: &[u8], raw| fs::read(tmp.path().join("in").join(OodleSpool::name(n, raw)));
        assert_eq!(input(b"one", 4).unwrap(), b"one");
        assert_eq!(input(b"three", 2).unwrap(), b"three");
        assert!(input(b"two", 4).is_err());
        // Other codecs queue nothing.
        blob[28] = 1;
        assert_eq!(s.spool_blob(&blob, 0).unwrap(), 0);
    }

    #[test]
    fn other_codecs_pass_through() {
        let (_tmp, s) = spool();
        assert_eq!(
            s.decompress_block(Codec::Stored, b"plain", 5, 0).unwrap(),
            b"plain"
        );
    }
}
