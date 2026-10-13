use std::fs::{self, File};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::hexdump::hexdump;

/// Hex-dump log of what one client connection sent (all of it, or, while holding, all but its
/// auth), and, with `--speak`, what the stub sent.
pub struct SessionLog {
    path: PathBuf,
    file: File,
    received: usize,
    sent: usize,
    /// While holding ([`SessionLog::hold`]): the client's bytes so far, not yet written, and how
    /// many arrived in all (the buffer stops at [`HOLD_MAX`]).
    held: Option<(Vec<u8>, usize)>,
}

/// The most client bytes a holding log buffers. A client's auth is far smaller; past this, the
/// log stops buffering and only counts.
const HOLD_MAX: usize = 1 << 20;

impl SessionLog {
    /// Creates `<dir>/<unix_millis>-<local_port>-<peer>.log`, creating `dir` if needed.
    /// Never overwrites an existing file.
    pub fn create(dir: &Path, local: SocketAddr, peer: SocketAddr) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        // ':' (IPv6, port separator) isn't portable in file names.
        let peer_name = peer.to_string().replace([':', '[', ']'], "_");
        let path = dir.join(format!("{millis}-{}-{peer_name}.log", local.port()));
        let mut file = File::options().write(true).create_new(true).open(&path)?;
        writeln!(file, "# fenji-stub session\n# local {local}\n# peer {peer}")?;
        file.flush()?;
        Ok(Self {
            path,
            file,
            received: 0,
            sent: 0,
            held: None,
        })
    }

    /// From now on, the client's bytes are kept back until [`SessionLog::release`]: they hold its
    /// auth, which can carry a real platform ticket (#102). Called before the first `record`, so
    /// the held bytes start at the stream's first byte, where the auth's count starts too.
    pub fn hold(&mut self) {
        debug_assert_eq!(self.received, 0, "hold before the first record");
        self.held = Some((Vec::new(), 0));
    }

    /// Whether client bytes are being kept back.
    pub fn holding(&self) -> bool {
        self.held.is_some()
    }

    /// Ends the hold: the first `skip` bytes held (the frames that carried the auth) become one
    /// marker line, and the rest is recorded as usual.
    pub fn release(&mut self, skip: usize) -> io::Result<()> {
        let Some((held, total)) = self.held.take() else {
            return Ok(());
        };
        let skip = skip.min(total);
        writeln!(self.file, "# [auth frames not captured: {skip} bytes]")?;
        self.received += skip;
        // Bytes past the buffer's limit were never kept, and can't be recorded now.
        let rest = held.get(skip..).unwrap_or(&[]);
        let lost = total - skip - rest.len();
        if !rest.is_empty() {
            self.record(rest)?;
        }
        if lost > 0 {
            writeln!(self.file, "# [{lost} more bytes not kept while holding]")?;
            self.received += lost;
        }
        self.file.flush()
    }

    /// Appends one read's bytes, offset-continuous with earlier reads; while holding, keeps them
    /// back instead.
    pub fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
        if let Some((held, total)) = &mut self.held {
            let room = HOLD_MAX.saturating_sub(held.len());
            held.extend_from_slice(&bytes[..bytes.len().min(room)]);
            *total += bytes.len();
            return Ok(());
        }
        writeln!(
            self.file,
            "# recv {} bytes at offset {}",
            bytes.len(),
            self.received
        )?;
        self.file
            .write_all(hexdump(self.received, bytes).as_bytes())?;
        self.received += bytes.len();
        self.file.flush()
    }

    /// Appends bytes the stub sent, offset-continuous with earlier sends.
    pub fn record_sent(&mut self, bytes: &[u8]) -> io::Result<()> {
        writeln!(
            self.file,
            "# sent {} bytes at offset {}",
            bytes.len(),
            self.sent
        )?;
        self.file.write_all(hexdump(self.sent, bytes).as_bytes())?;
        self.sent += bytes.len();
        self.file.flush()
    }

    /// Writes the closing line. `reason` is e.g. "peer closed" or the read error. Bytes still held
    /// (the auth never completed) are counted, never written.
    pub fn close(mut self, reason: &str) -> io::Result<()> {
        if let Some((_, total)) = self.held.take() {
            writeln!(
                self.file,
                "# [{total} bytes before a complete auth not captured]"
            )?;
            self.received += total;
        }
        writeln!(
            self.file,
            "# closed after {} bytes: {reason}",
            self.received
        )?;
        self.file.flush()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn received(&self) -> usize {
        self.received
    }
}

#[cfg(test)]
mod tests {
    use super::SessionLog;
    use std::fs;

    fn addrs() -> (std::net::SocketAddr, std::net::SocketAddr) {
        (
            "127.0.0.1:8888".parse().unwrap(),
            "10.0.0.5:51234".parse().unwrap(),
        )
    }

    #[test]
    fn creates_missing_directory_and_header() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("nested/logs");
        let (local, peer) = addrs();
        let log = SessionLog::create(&dir, local, peer).unwrap();
        let name = log
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.ends_with("-8888-10.0.0.5_51234.log"), "{name}");
        let text = fs::read_to_string(log.path()).unwrap();
        assert!(text.contains("# local 127.0.0.1:8888\n# peer 10.0.0.5:51234\n"));
    }

    #[test]
    fn records_reads_with_continuous_offsets_and_close_line() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.record(b"abc").unwrap();
        log.record(b"de").unwrap();
        assert_eq!(log.received(), 5);
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("# recv 3 bytes at offset 0\n00000000  61 62 63"));
        assert!(text.contains("# recv 2 bytes at offset 3\n00000003  64 65"));
        assert!(text.ends_with("# closed after 5 bytes: peer closed\n"));
    }

    #[test]
    fn sent_bytes_have_their_own_offsets() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.record_sent(b"hi").unwrap();
        log.record(b"abc").unwrap();
        log.record_sent(b"!").unwrap();
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("# sent 2 bytes at offset 0\n00000000  68 69"));
        assert!(text.contains("# recv 3 bytes at offset 0\n00000000  61 62 63"));
        assert!(text.contains("# sent 1 bytes at offset 2\n00000002  21"));
    }

    /// The second read mixes the auth's last byte with the first bytes after it, as one read that
    /// holds the end of the auth's frames and the next frame does (#145's review).
    #[test]
    fn a_held_auth_is_never_written() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.hold();
        log.record(b"AUTH").unwrap();
        log.record(b"Hxy").unwrap();
        log.release(5).unwrap();
        log.record(b"z").unwrap();
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(
            text.contains("# [auth frames not captured: 5 bytes]"),
            "{text}"
        );
        assert!(
            text.contains("# recv 2 bytes at offset 5\n00000005  78 79"),
            "{text}"
        );
        assert!(text.contains("# recv 1 bytes at offset 7"), "{text}");
        assert!(
            !text.contains("41 55 54 48"),
            "the held bytes stay out: {text}"
        );
        assert!(
            text.ends_with("# closed after 8 bytes: peer closed\n"),
            "{text}"
        );
    }

    #[test]
    fn a_close_before_the_auth_completes_counts_and_writes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.hold();
        log.record(b"AUT").unwrap();
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("# recv"), "{text}");
        assert!(
            text.contains("# [3 bytes before a complete auth not captured]"),
            "{text}"
        );
        assert!(
            text.ends_with("# closed after 3 bytes: peer closed\n"),
            "{text}"
        );
    }

    #[test]
    fn a_hold_past_its_limit_counts_what_it_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.hold();
        log.record(&vec![0u8; super::HOLD_MAX + 10]).unwrap();
        log.release(super::HOLD_MAX - 4).unwrap();
        assert_eq!(log.received(), super::HOLD_MAX + 10);
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("# recv 4 bytes at offset"));
        assert!(text.contains("# [10 more bytes not kept while holding]"));
    }

    #[test]
    fn an_auth_longer_than_the_buffer_counts_what_it_never_kept() {
        // The auth ends past HOLD_MAX: skip is beyond what the buffer holds, so nothing is
        // recorded, and the bytes after the auth that weren't kept are counted.
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.hold();
        log.record(&vec![7u8; super::HOLD_MAX + 10]).unwrap();
        log.release(super::HOLD_MAX + 5).unwrap();
        assert_eq!(log.received(), super::HOLD_MAX + 10);
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("# recv"), "{text}");
        assert!(
            text.contains(&format!(
                "# [auth frames not captured: {} bytes]",
                super::HOLD_MAX + 5
            )),
            "{text}"
        );
        assert!(
            text.contains("# [5 more bytes not kept while holding]"),
            "{text}"
        );
    }

    #[test]
    fn a_close_while_holding_past_the_limit_counts_every_byte() {
        let tmp = tempfile::tempdir().unwrap();
        let (local, peer) = addrs();
        let mut log = SessionLog::create(tmp.path(), local, peer).unwrap();
        log.hold();
        log.record(&vec![7u8; super::HOLD_MAX + 10]).unwrap();
        let path = log.path().to_path_buf();
        log.close("peer closed").unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(!text.contains("# recv"), "{text}");
        let all = super::HOLD_MAX + 10;
        assert!(
            text.contains(&format!(
                "# [{all} bytes before a complete auth not captured]"
            )),
            "{text}"
        );
        assert!(
            text.ends_with(&format!("# closed after {all} bytes: peer closed\n")),
            "{text}"
        );
    }

    #[test]
    fn ipv6_peer_produces_portable_file_name() {
        let tmp = tempfile::tempdir().unwrap();
        let local = "[::1]:9000".parse().unwrap();
        let peer = "[::1]:40000".parse().unwrap();
        let log = SessionLog::create(tmp.path(), local, peer).unwrap();
        let name = log
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(!name.contains(':') && !name.contains('['), "{name}");
    }
}
