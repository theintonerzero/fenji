//! The parts of a PE32+ image the readers need: its sections and the mapping from virtual
//! addresses to file bytes. Only file-backed bytes are readable, and nothing is executed.
use crate::{Error, Result};

/// One section of the image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    /// Absolute virtual address: image base + RVA.
    pub va: u64,
    pub raw_offset: usize,
    pub raw_size: usize,
}

/// A parsed PE32+ image that owns its file bytes.
#[derive(Debug)]
pub struct PeImage {
    bytes: Vec<u8>,
    image_base: u64,
    sections: Vec<Section>,
}

fn le<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..at.checked_add(N)?)?.try_into().ok()
}

impl PeImage {
    /// Parses the DOS, COFF and optional headers and the section table.
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        if bytes.get(..2) != Some(b"MZ".as_slice()) {
            return Err(Error::NotPe("no MZ header"));
        }
        let pe = u32::from_le_bytes(le(&bytes, 0x3C).ok_or(Error::NotPe("truncated DOS header"))?)
            as usize;
        if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0".as_slice()) {
            return Err(Error::NotPe("no PE signature"));
        }
        let count =
            u16::from_le_bytes(le(&bytes, pe + 6).ok_or(Error::NotPe("truncated COFF header"))?)
                as usize;
        let opt_size =
            u16::from_le_bytes(le(&bytes, pe + 20).ok_or(Error::NotPe("truncated COFF header"))?)
                as usize;
        let opt = pe + 24;
        if le::<2>(&bytes, opt).map(u16::from_le_bytes) != Some(0x20B) {
            return Err(Error::NotPe("not PE32+"));
        }
        let image_base = u64::from_le_bytes(
            le(&bytes, opt + 24).ok_or(Error::NotPe("truncated optional header"))?,
        );
        let mut sections = Vec::with_capacity(count);
        for i in 0..count {
            let at = opt + opt_size + 40 * i;
            let raw = bytes
                .get(at..at + 40)
                .ok_or(Error::NotPe("truncated section table"))?;
            let field = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().expect("4 bytes"));
            sections.push(Section {
                name: String::from_utf8_lossy(&raw[..8])
                    .trim_end_matches('\0')
                    .to_string(),
                va: image_base + u64::from(field(12)),
                raw_size: field(16) as usize,
                raw_offset: field(20) as usize,
            });
        }
        Ok(Self {
            bytes,
            image_base,
            sections,
        })
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The section whose file-backed bytes contain `va`.
    pub fn section_of(&self, va: u64) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| va >= s.va && va - s.va < s.raw_size as u64)
    }

    /// A section's file bytes (clipped to the file).
    pub fn section_data(&self, s: &Section) -> &[u8] {
        let end = s
            .raw_offset
            .saturating_add(s.raw_size)
            .min(self.bytes.len());
        self.bytes
            .get(s.raw_offset.min(end)..end)
            .unwrap_or_default()
    }

    /// The file bytes behind `len` bytes at `va`; `None` unless all lie in one section's file data.
    pub fn read(&self, va: u64, len: usize) -> Option<&[u8]> {
        let s = self.section_of(va)?;
        let off = usize::try_from(va - s.va).ok()?;
        if off.checked_add(len)? > s.raw_size {
            return None;
        }
        let start = s.raw_offset.checked_add(off)?;
        self.bytes.get(start..start.checked_add(len)?)
    }

    /// The NUL-terminated string at `va` (without its NUL), at most `max` bytes long.
    pub fn c_str(&self, va: u64, max: usize) -> Option<&[u8]> {
        let s = self.section_of(va)?;
        let off = usize::try_from(va - s.va).ok()?;
        let data = self.section_data(s).get(off..)?;
        let end = data
            .iter()
            .take(max.saturating_add(1))
            .position(|&b| b == 0)?;
        Some(&data[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{IMAGE_BASE, pe_image};

    #[test]
    fn maps_virtual_addresses_to_file_bytes() {
        let img = PeImage::parse(pe_image(&[
            (".rdata", 0x1000, b"abc\0def".to_vec()),
            (".data", 0x2000, vec![9; 16]),
        ]))
        .unwrap();
        assert_eq!(img.image_base(), IMAGE_BASE);
        assert_eq!(img.read(IMAGE_BASE + 0x1004, 3), Some(b"def".as_slice()));
        assert_eq!(img.c_str(IMAGE_BASE + 0x1000, 16), Some(b"abc".as_slice()));
        assert_eq!(
            img.section_of(IMAGE_BASE + 0x2000).map(|s| s.name.as_str()),
            Some(".data")
        );
    }

    #[test]
    fn read_outside_image_is_none() {
        let img = PeImage::parse(pe_image(&[(".rdata", 0x1000, vec![1; 8])])).unwrap();
        assert_eq!(img.read(IMAGE_BASE + 0x1006, 4), None); // runs past the section's file data
        assert_eq!(img.read(IMAGE_BASE + 0x5000, 1), None); // in no section
        assert_eq!(img.read(u64::MAX - 1, 4), None);
        assert_eq!(img.c_str(IMAGE_BASE + 0x1000, 16), None); // no NUL before the end
    }

    #[test]
    fn rejects_what_is_not_pe32_plus() {
        assert!(matches!(
            PeImage::parse(b"not a PE".to_vec()),
            Err(Error::NotPe(_))
        ));
        let mut pe = pe_image(&[(".rdata", 0x1000, vec![0; 4])]);
        pe[0x58..0x5A].copy_from_slice(&0x10Bu16.to_le_bytes()); // PE32, not PE32+
        assert!(matches!(PeImage::parse(pe), Err(Error::NotPe("not PE32+"))));
    }
}
