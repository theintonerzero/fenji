use std::fs::{self, File};
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::hexdump::hexdump;

/// Hex-dump log of everything one client connection sent.
pub struct SessionLog {
    path: PathBuf,
    file: File,
    received: usize,
}

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
        })
    }

    /// Appends one read's bytes, offset-continuous with earlier reads.
    pub fn record(&mut self, bytes: &[u8]) -> io::Result<()> {
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

    /// Writes the closing line. `reason` is e.g. "peer closed" or the read error.
    pub fn close(mut self, reason: &str) -> io::Result<()> {
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
