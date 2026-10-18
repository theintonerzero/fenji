//! A pack: a gzipped tar of `manifest.json`, `text/<language>/<area>.tsv` and `glossary.tsv`,
//! signed with minisign (retranslation spec §6.2, RT4).
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Fingerprint, PackError, read_tsv};

/// The pack format this crate reads.
pub const FORMAT: u32 = 1;
/// The largest pack read: the whole English is about 17 MB before compression.
pub const MAX_PACK: u64 = 256 << 20;
/// The most a pack may unpack to.
pub const MAX_UNPACKED: u64 = 1 << 30;

/// What a pack says of itself, as `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    /// The client build its fingerprints were taken from, such as `10015727`.
    pub build: String,
    /// The content release, such as `v1.0.0`.
    pub version: String,
    /// How many lines of English its text files hold, in all.
    pub lines: u64,
    /// Every other file in the pack, with its SHA-256 in lowercase hex.
    pub files: BTreeMap<String, String>,
}

/// A pack that was read and checked: its manifest, and every line's English by fingerprint.
#[derive(Debug)]
pub struct Pack {
    pub manifest: Manifest,
    pub english: HashMap<Fingerprint, String>,
}

fn refused(why: impl Into<String>) -> PackError {
    PackError::Pack(why.into())
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A name the format allows: lowercase letters, digits, `_` and `-`.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// The entries a pack may hold, by plain names only: no `.`, `..` or absolute path gets through.
fn allowed(path: &str) -> bool {
    match path.split('/').collect::<Vec<_>>().as_slice() {
        ["manifest.json"] | ["glossary.tsv"] => true,
        ["text", language, file] => plain(language) && file.strip_suffix(".tsv").is_some_and(plain),
        _ => false,
    }
}

/// Reads `bytes`, a pack, and checks it whole: only the entries the format has, each a regular
/// file, at most [`MAX_UNPACKED`] in all; the manifest's format; every file the manifest names
/// there with its SHA-256, and no other; every text file's entries readable, no fingerprint in
/// two of them, and as many lines as the manifest says.
pub fn read_pack(bytes: &[u8]) -> Result<Pack, PackError> {
    if bytes.len() as u64 > MAX_PACK {
        return Err(refused(format!("{} bytes, over {MAX_PACK}", bytes.len())));
    }
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut manifest: Option<Manifest> = None;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut unpacked = 0u64;
    for entry in archive.entries()? {
        let entry = entry?;
        let path = String::from_utf8(entry.path_bytes().into_owned())
            .map_err(|_| refused("an entry's name isn't UTF-8"))?;
        if !allowed(&path) {
            return Err(refused(format!(
                "an entry the format doesn't have: {path:?}"
            )));
        }
        if entry.header().entry_type() != tar::EntryType::Regular {
            return Err(refused(format!("{path} isn't a regular file")));
        }
        let size = entry.header().size()?;
        unpacked = unpacked.saturating_add(size);
        if unpacked > MAX_UNPACKED {
            return Err(refused(format!(
                "unpacks to more than {MAX_UNPACKED} bytes"
            )));
        }
        let mut data = Vec::new();
        entry.take(size).read_to_end(&mut data)?;
        if path == "manifest.json" {
            if manifest.is_some() {
                return Err(refused("two manifests"));
            }
            manifest = Some(
                serde_json::from_slice(&data)
                    .map_err(|e| refused(format!("manifest.json: {e}")))?,
            );
        } else if files.insert(path.clone(), data).is_some() {
            return Err(refused(format!("{path} twice")));
        }
    }
    let manifest = manifest.ok_or_else(|| refused("no manifest.json"))?;
    if manifest.format != FORMAT {
        return Err(refused(format!(
            "format {}, where this launcher reads {FORMAT}",
            manifest.format
        )));
    }
    let named: BTreeSet<&String> = manifest.files.keys().collect();
    let present: BTreeSet<&String> = files.keys().collect();
    if named != present {
        return Err(refused(format!(
            "the manifest names {named:?}, the pack holds {present:?}"
        )));
    }
    let mut english = HashMap::new();
    for (path, data) in &files {
        if manifest.files[path] != sha256_hex(data) {
            return Err(refused(format!("{path} isn't what the manifest says")));
        }
        if path == "glossary.tsv" {
            continue;
        }
        let text = std::str::from_utf8(data).map_err(|_| refused(format!("{path} isn't UTF-8")))?;
        for e in read_tsv(text).map_err(|e| refused(format!("{path}: {e}")))? {
            if english.insert(e.fingerprint, e.english).is_some() {
                return Err(refused(format!("{} is in two files", e.fingerprint)));
            }
        }
    }
    if english.len() as u64 != manifest.lines {
        return Err(refused(format!(
            "{} lines, where the manifest says {}",
            english.len(),
            manifest.lines
        )));
    }
    Ok(Pack { manifest, english })
}

/// Checks `signature`, a minisign signature file's text, on `pack`, against `public_key`, a
/// minisign public key's base64 line. Only prehashed signatures, as minisign makes today.
pub fn verify(pack: &[u8], signature: &str, public_key: &str) -> Result<(), PackError> {
    let refused =
        |what: &str, e: minisign_verify::Error| PackError::Signature(format!("{what}: {e}"));
    let key = minisign_verify::PublicKey::from_base64(public_key.trim())
        .map_err(|e| refused("the key", e))?;
    let signature =
        minisign_verify::Signature::decode(signature).map_err(|e| refused("the signature", e))?;
    key.verify(pack, &signature, false)
        .map_err(|e| refused("the pack", e))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;
    use crate::{Entry, fingerprint, write_tsv};

    /// A `.tar.gz` of `files`, in their order, each path written as it is, unchecked: the reader
    /// is what's tested.
    fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (path, bytes) in files {
            let mut header = tar::Header::new_old();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.as_old_mut().name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            builder.append(&header, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// A pack whose one text file is a symbolic link, empty, as its manifest says.
    fn linked() -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let files = [("text/en/items.tsv", &b""[..])];
        let named = manifest(&files, 0);
        let mut header = tar::Header::new_old();
        header.set_size(named.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path("manifest.json").unwrap();
        header.set_cksum();
        builder.append(&header, &named[..]).unwrap();
        let mut link = tar::Header::new_old();
        link.set_size(0);
        link.set_entry_type(tar::EntryType::Symlink);
        link.set_path(files[0].0).unwrap();
        link.set_link_name("/etc/passwd").unwrap();
        link.set_cksum();
        builder.append(&link, std::io::empty()).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn manifest_of_format(format: u32, files: &[(&str, &[u8])], lines: u64) -> Vec<u8> {
        let mut m: Manifest = serde_json::from_slice(&manifest(files, lines)).unwrap();
        m.format = format;
        serde_json::to_vec(&m).unwrap()
    }

    /// Invented lines, not the game's.
    fn items() -> String {
        let mut entries = vec![
            Entry {
                fingerprint: fingerprint("剑".as_bytes()),
                english: "Sword".into(),
            },
            Entry {
                fingerprint: fingerprint("盾".as_bytes()),
                english: "Shield".into(),
            },
        ];
        write_tsv(&mut entries)
    }

    fn manifest(files: &[(&str, &[u8])], lines: u64) -> Vec<u8> {
        let m = Manifest {
            format: FORMAT,
            build: "10015727".into(),
            version: "v0.0.1".into(),
            lines,
            files: files
                .iter()
                .map(|(p, b)| (p.to_string(), sha256_hex(b)))
                .collect(),
        };
        serde_json::to_vec(&m).unwrap()
    }

    fn good() -> Vec<u8> {
        let items = items();
        let files = [("text/en/items.tsv", items.as_bytes())];
        tarball(&[("manifest.json", &manifest(&files, 2)), files[0]])
    }

    #[test]
    fn a_pack_is_read_only_when_every_file_matches_its_manifest() {
        let pack = read_pack(&good()).unwrap();
        assert_eq!(pack.manifest.build, "10015727");
        assert_eq!(pack.english.len(), 2);
        assert_eq!(pack.english[&fingerprint("剑".as_bytes())], "Sword");

        let items = items();
        let files = [("text/en/items.tsv", items.as_bytes())];
        let named = manifest(&files, 2);
        let changed = items.replace("Sword", "Sward");
        let twice = [
            ("text/en/items.tsv", items.as_bytes()),
            ("text/en/lore.tsv", items.as_bytes()),
        ];
        // Each case is refused by its own check alone: the escaping path is named in its
        // manifest, and the repeated lines are counted once.
        let escaping = [("../items.tsv", items.as_bytes())];
        let bad: Vec<(&str, Vec<u8>)> = vec![
            (
                "a changed byte",
                tarball(&[
                    ("manifest.json", &named),
                    ("text/en/items.tsv", changed.as_bytes()),
                ]),
            ),
            (
                "an extra entry",
                tarball(&[
                    ("manifest.json", &named),
                    files[0],
                    ("text/en/more.tsv", items.as_bytes()),
                ]),
            ),
            (
                "a ../ path",
                tarball(&[("manifest.json", &manifest(&escaping, 2)), escaping[0]]),
            ),
            (
                "a fingerprint in two files",
                tarball(&[("manifest.json", &manifest(&twice, 2)), twice[0], twice[1]]),
            ),
            (
                "lines not the count",
                tarball(&[("manifest.json", &manifest(&files, 3)), files[0]]),
            ),
            ("no manifest", tarball(&[files[0]])),
            ("a link for a file", linked()),
            (
                "another format",
                tarball(&[
                    ("manifest.json", &manifest_of_format(2, &files, 2)),
                    files[0],
                ]),
            ),
            (
                "a file the manifest names, missing",
                tarball(&[("manifest.json", &named)]),
            ),
        ];
        for (why, bytes) in bad {
            assert!(read_pack(&bytes).is_err(), "{why}");
        }
    }

    #[test]
    fn a_pack_is_verified_by_its_minisign_signature() {
        let pack = good();
        let sign = |key: &minisign::KeyPair| {
            String::from(
                minisign::sign(Some(&key.pk), &key.sk, Cursor::new(&pack), None, None).unwrap(),
            )
        };
        let key = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public = key.pk.to_base64();
        let signature = sign(&key);
        verify(&pack, &signature, &public).unwrap();

        let mut changed = pack.clone();
        changed[20] ^= 1;
        assert!(verify(&changed, &signature, &public).is_err());

        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        assert!(verify(&pack, &sign(&other), &public).is_err());
    }
}
