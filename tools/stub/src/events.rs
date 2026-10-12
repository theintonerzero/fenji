//! The decoded side of a `--speak` session: one tab-separated line per event, `millis kind detail`,
//! with the time since the connection opened. The raw hexdump log stays the ground truth.
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

pub struct EventLog {
    file: File,
    start: Instant,
}

impl EventLog {
    /// Creates `path`; never overwrites.
    pub fn create(path: &Path) -> io::Result<Self> {
        let file = File::options().write(true).create_new(true).open(path)?;
        Ok(Self {
            file,
            start: Instant::now(),
        })
    }

    pub fn line(&mut self, kind: &str, detail: &str) -> io::Result<()> {
        let ms = self.start.elapsed().as_millis();
        writeln!(self.file, "{ms}\t{kind}\t{detail}")?;
        self.file.flush()
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
