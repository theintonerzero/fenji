//! The two programs the launcher fetches on first use (launcher spec §3.6, §3.7; L4): DepotDownloader
//! and umu-launcher, from their GitHub releases at pinned versions, each checked against a SHA-256
//! before anything of it is unpacked. Only the one program each archive is fetched for is unpacked;
//! neither ships with the launcher.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use sha2::{Digest, Sha256};

/// The largest download, and the largest program unpacked from it.
pub const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;

/// How a tool's release is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    Zip,
    Tar,
}

/// A program fetched on first use: where from, its pinned digest, and the one file it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub version: String,
    /// `https://`, or `http://` on this machine (for tests).
    pub url: String,
    pub sha256: [u8; 32],
    pub archive: Archive,
    /// The program's path inside the archive; also its path under the tool's folder.
    pub member: String,
}

/// Why a tool couldn't be made ready. The caller names the tool.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("the download failed: {0}")]
    Download(String),
    #[error("the download was larger than any release of it")]
    TooLarge,
    #[error("the download didn't match its pinned checksum")]
    Checksum,
    #[error("the download wasn't the archive expected: {0}")]
    Archive(String),
    #[error("it couldn't be saved: {0}")]
    Io(#[from] std::io::Error),
}

/// DepotDownloader for Linux x64, as `scripts/versions.env` pins it.
pub fn depot_downloader() -> Tool {
    Tool {
        name: "DepotDownloader".into(),
        version: "3.4.0".into(),
        url: "https://github.com/SteamRE/DepotDownloader/releases/download/DepotDownloader_3.4.0/DepotDownloader-linux-x64.zip".into(),
        sha256: digest("a999dec66b4850fc961bd50366696d23c2d0fad7b18790e6a5647b2f19097a53"),
        archive: Archive::Zip,
        member: "DepotDownloader".into(),
    }
}

/// umu-launcher's zipapp, as `scripts/versions.env` pins it.
pub fn umu_launcher() -> Tool {
    Tool {
        name: "umu-launcher".into(),
        version: "1.4.4".into(),
        url: "https://github.com/Open-Wine-Components/umu-launcher/releases/download/1.4.4/umu-launcher-1.4.4-zipapp.tar".into(),
        sha256: digest("eb590691841f7fad3fc3ad8fd5db4ccb87849fe7948e62b28ece7a4ee48cc851"),
        archive: Archive::Tar,
        member: "umu/umu-run".into(),
    }
}

/// A pinned digest from its 64 hex digits.
fn digest(hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("a pinned digest is hex");
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The tool's folder under `dir`: `<name>-<version>`.
fn home(tool: &Tool, dir: &Path) -> PathBuf {
    dir.join(format!("{}-{}", tool.name, tool.version))
}

/// The program's path, when a finished install of exactly this tool is there: its marker holds the
/// pinned digest, and the program is an executable file.
pub fn installed(tool: &Tool, dir: &Path) -> Option<PathBuf> {
    let home = home(tool, dir);
    let marker = std::fs::read_to_string(home.join(".sha256")).ok()?;
    if marker.trim() != hex(&tool.sha256) {
        return None;
    }
    let program = home.join(&tool.member);
    let meta = std::fs::metadata(&program).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(program)
}

/// The program's path: [`installed`], or else downloaded, checked against its digest, and only
/// its member unpacked, into `dir`. `progress` hears the bytes so far and the total, when known.
pub async fn ensure(
    tool: &Tool,
    dir: &Path,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<PathBuf, ToolError> {
    ensure_capped(tool, dir, progress, MAX_DOWNLOAD).await
}

/// [`ensure`] with `cap` in place of [`MAX_DOWNLOAD`].
pub async fn ensure_capped(
    tool: &Tool,
    dir: &Path,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    cap: u64,
) -> Result<PathBuf, ToolError> {
    if let Some(program) = installed(tool, dir) {
        return Ok(program);
    }
    let url = Url::parse(&tool.url).map_err(|e| ToolError::Download(e.to_string()))?;
    let local = match url.scheme() {
        "https" => false,
        "http" if crate::api::loopback(&url) => true,
        _ => {
            return Err(ToolError::Download(
                "its address must start with https://".into(),
            ));
        }
    };
    std::fs::create_dir_all(dir)?;
    let stem = format!("{}-{}", tool.name, tool.version);
    let part = dir.join(format!(".{stem}.part"));
    let new = dir.join(format!(".{stem}.new"));
    let result = fetch(&url, local, &part, cap, progress)
        .await
        .and_then(|sum| {
            if sum == tool.sha256 {
                Ok(())
            } else {
                Err(ToolError::Checksum)
            }
        });
    let result = match result {
        Ok(()) => {
            let (tool, part, new, home) =
                (tool.clone(), part.clone(), new.clone(), home(tool, dir));
            tokio::task::spawn_blocking(move || unpack(&tool, &part, &new, &home, cap))
                .await
                .map_err(|e| ToolError::Io(std::io::Error::other(e)))?
        }
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_file(&part);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&new);
    }
    result
}

/// Streams `url` into `part`, never past `cap` bytes, and returns the SHA-256 of what came.
async fn fetch(
    url: &Url,
    local: bool,
    part: &Path,
    cap: u64,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<[u8; 32], ToolError> {
    crate::api::crypto();
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        // GitHub's release downloads redirect; never off https, and never far.
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() == "https" {
                attempt.follow()
            } else {
                attempt.error("a redirect left https")
            }
        }));
    if local {
        builder = builder.no_proxy();
    }
    let client = builder
        .build()
        .map_err(|e| ToolError::Download(e.to_string()))?;
    let failed = |e: reqwest::Error| ToolError::Download(e.without_url().to_string());
    let mut res = client.get(url.clone()).send().await.map_err(failed)?;
    if !res.status().is_success() {
        return Err(ToolError::Download(format!(
            "the server answered {}",
            res.status()
        )));
    }
    let total = res.content_length();
    if total.is_some_and(|t| t > cap) {
        return Err(ToolError::TooLarge);
    }
    let mut out = std::fs::File::create(part)?;
    let mut sum = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = res.chunk().await.map_err(failed)? {
        got += chunk.len() as u64;
        if got > cap {
            return Err(ToolError::TooLarge);
        }
        sum.update(&chunk);
        out.write_all(&chunk)?;
        progress(got, total);
    }
    out.sync_all()?;
    Ok(sum.finalize().into())
}

/// Unpacks only `tool.member` from `part` into a fresh `new`, makes it executable, marks the
/// folder with the digest, and moves it to `home`. Returns the program's path.
fn unpack(
    tool: &Tool,
    part: &Path,
    new: &Path,
    home: &Path,
    cap: u64,
) -> Result<PathBuf, ToolError> {
    if new.exists() {
        std::fs::remove_dir_all(new)?;
    }
    let target = new.join(&tool.member);
    std::fs::create_dir_all(target.parent().unwrap_or(new))?;
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    let copied = match tool.archive {
        Archive::Zip => {
            let mut zip = zip::ZipArchive::new(std::fs::File::open(part)?)
                .map_err(|e| ToolError::Archive(e.to_string()))?;
            let entry = zip
                .by_name(&tool.member)
                .map_err(|_| ToolError::Archive(format!("it holds no {}", tool.member)))?;
            if !entry.is_file() {
                return Err(ToolError::Archive(format!("{} isn't a file", tool.member)));
            }
            copy_capped(entry, &mut out, cap)?
        }
        Archive::Tar => {
            let mut tar = tar::Archive::new(std::fs::File::open(part)?);
            let mut copied = None;
            for entry in tar.entries()? {
                let entry = entry?;
                let path = entry.path()?.into_owned();
                let path = path.strip_prefix(".").unwrap_or(&path);
                if path == Path::new(&tool.member) && entry.header().entry_type().is_file() {
                    copied = Some(copy_capped(entry, &mut out, cap)?);
                    break;
                }
            }
            copied.ok_or_else(|| ToolError::Archive(format!("it holds no {}", tool.member)))?
        }
    };
    if copied == 0 {
        return Err(ToolError::Archive(format!("{} is empty", tool.member)));
    }
    out.sync_all()?;
    drop(out);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
    }
    std::fs::write(new.join(".sha256"), hex(&tool.sha256))?;
    if home.exists() {
        std::fs::remove_dir_all(home)?;
    }
    std::fs::rename(new, home)?;
    Ok(home.join(&tool.member))
}

/// Copies at most `cap` bytes, and refuses more: a small archive can't unpack into a huge file.
fn copy_capped(from: impl Read, to: &mut std::fs::File, cap: u64) -> Result<u64, ToolError> {
    let mut limited = from.take(cap + 1);
    let n = std::io::copy(&mut limited, to)?;
    if n > cap {
        return Err(ToolError::TooLarge);
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn serve(bytes: Vec<u8>) -> String {
        let app = axum::Router::new().route(
            "/t",
            axum::routing::get(move || {
                let body = bytes.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}/t")
    }

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut out);
        for (name, body) in entries {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut z, body).unwrap();
        }
        z.finish().unwrap();
        out.into_inner()
    }

    fn tool(url: String, body: &[u8], archive: Archive, member: &str) -> Tool {
        use sha2::{Digest, Sha256};
        Tool {
            name: "Thing".into(),
            version: "1.0".into(),
            url,
            sha256: Sha256::digest(body).into(),
            archive,
            member: member.into(),
        }
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[tokio::test]
    async fn a_tool_is_fetched_checked_and_only_its_member_unpacked() {
        let body = zip_with(&[
            ("Thing", b"#!/bin/sh\necho hi\n"),
            ("LICENSE", b"text"),
            ("../evil", b"x"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let t = tool(serve(body.clone()).await, &body, Archive::Zip, "Thing");
        let path = ensure(&t, dir.path(), &|_, _| {}).await.unwrap();
        assert_eq!(path, dir.path().join("Thing-1.0/Thing"));
        assert_eq!(std::fs::read(&path).unwrap(), b"#!/bin/sh\necho hi\n");
        #[cfg(unix)]
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(
                &std::fs::metadata(&path).unwrap().permissions()
            ) & 0o777,
            0o755
        );
        assert!(
            !dir.path().join("Thing-1.0/LICENSE").exists() && !dir.path().join("evil").exists()
        );
        assert_eq!(installed(&t, dir.path()), Some(path));
    }

    #[tokio::test]
    async fn a_wrong_checksum_leaves_nothing_behind() {
        let body = zip_with(&[("Thing", b"x")]);
        let dir = tempfile::tempdir().unwrap();
        let mut t = tool(serve(body.clone()).await, &body, Archive::Zip, "Thing");
        t.sha256 = [0; 32];
        assert!(matches!(
            ensure(&t, dir.path(), &|_, _| {}).await,
            Err(ToolError::Checksum)
        ));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "a .part or .new was left"
        );
    }

    #[tokio::test]
    async fn a_tar_member_in_a_folder_unpacks_and_a_missing_member_is_an_archive_error() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "umu/umu-run", &b"zip"[..])
            .unwrap();
        let body = builder.into_inner().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let t = tool(
            serve(body.clone()).await,
            &body,
            Archive::Tar,
            "umu/umu-run",
        );
        assert_eq!(
            ensure(&t, dir.path(), &|_, _| {}).await.unwrap(),
            dir.path().join("Thing-1.0/umu/umu-run")
        );
        let wrong = tool(
            serve(body.clone()).await,
            &body,
            Archive::Tar,
            "umu/nothing",
        );
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(matches!(
            ensure(&wrong, elsewhere.path(), &|_, _| {}).await,
            Err(ToolError::Archive(_))
        ));
        assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn a_download_over_its_cap_is_refused_and_leaves_nothing_behind() {
        let body = vec![7u8; 2048];
        let dir = tempfile::tempdir().unwrap();
        let t = tool(serve(body.clone()).await, &body, Archive::Zip, "Thing");
        assert!(matches!(
            ensure_capped(&t, dir.path(), &|_, _| {}, 1024).await,
            Err(ToolError::TooLarge)
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn plain_http_is_only_for_this_machine() {
        let dir = tempfile::tempdir().unwrap();
        let t = tool("http://example.com/t".into(), b"", Archive::Zip, "Thing");
        let err = ensure(&t, dir.path(), &|_, _| {}).await.unwrap_err();
        assert!(matches!(err, ToolError::Download(_)), "{err}");
        assert!(err.to_string().contains("https"), "{err}");
    }

    #[test]
    fn the_pinned_tools_are_the_ones_in_versions_env() {
        let env = include_str!("../../../scripts/versions.env");
        let d = depot_downloader();
        assert!(
            d.url
                .contains("DepotDownloader_3.4.0/DepotDownloader-linux-x64.zip")
        );
        assert!(env.contains(&format!(
            "DEPOTDOWNLOADER_SHA256_linux_x64=\"{}\"",
            hex(&d.sha256)
        )));
        assert_eq!(d.member, "DepotDownloader");
        let u = umu_launcher();
        assert!(u.url.ends_with("/1.4.4/umu-launcher-1.4.4-zipapp.tar"));
        assert!(env.contains(&format!("UMU_LAUNCHER_SHA256=\"{}\"", hex(&u.sha256))));
        assert_eq!(u.member, "umu/umu-run");
    }
}
