//! The latest GE-Proton for umu-run (launcher spec Amendment B, LB1; L23): GloriousEggroll's
//! `proton-ge-custom`, the Proton family Fenji's client runs use. Before each start the launcher
//! asks GitHub for its latest release. A release newer than the build in the launcher's own folder
//! is fetched, its archive checked against the release's own SHA-512 sum before anything of it is
//! unpacked, unpacked under the release's name, and the older build removed. Offline, or when
//! GitHub doesn't answer, the build it has plays; with none, umu-run's own default. A player's own
//! `PROTONPATH` asks nothing (the caller's choice).

use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reqwest::Url;
use sha2::{Digest, Sha512};
use tracing::{info, warn};

/// GE-Proton's latest release, as GitHub's API gives it.
pub const GE_RELEASES: &str =
    "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest";
/// The build fetched: the client is x86-64, and Play runs it on x86-64 Linux. A release's archive
/// is `<release>-x86_64.tar.gz`, whose one folder is `<release>-x86_64/`, beside its sum,
/// `<release>-x86_64.sha512sum` (GE-Proton 11-7, which the client runs pin, to 11-9 at least).
const BUILD: &str = "x86_64";
/// The largest archive fetched: GE-Proton 11-9's is 565 MB.
pub const MAX_ARCHIVE: u64 = 2 << 30;
/// The most an archive may unpack to: GE-Proton 11-7 unpacks to 1.6 GB.
pub const MAX_UNPACKED: u64 = 8 << 30;
/// The largest API answer or sum file read.
const MAX_SMALL: u64 = 1 << 20;
/// How long GitHub has to name its latest release before the build there plays.
const ASK_LIMIT: Duration = Duration::from_secs(10);
/// How often a fetch looks for its stop.
const LOOK_EVERY: Duration = Duration::from_millis(100);
/// What a finished build holds beside its files: its archive's SHA-512, written last.
pub const MARKER: &str = ".fenji-sha512";

/// Whether to stop (Play's Cancel start). Looked at often, by the unpacking thread too.
pub type Stop = Arc<dyn Fn() -> bool + Send + Sync>;

/// Why the latest release couldn't be had. The build already there plays instead.
#[derive(Debug, thiserror::Error)]
pub enum ProtonError {
    #[error("the download failed: {0}")]
    Download(String),
    #[error("the release isn't GE-Proton's: {0}")]
    Release(String),
    #[error("the download was larger than any release of it")]
    TooLarge,
    #[error("the download didn't match its release's checksum")]
    Checksum,
    #[error("the archive wasn't GE-Proton's: {0}")]
    Archive(String),
    #[error("it was cancelled")]
    Cancelled,
    #[error("it couldn't be saved: {0}")]
    Io(#[from] std::io::Error),
}

/// A release: its name (`GE-Proton11-9`), which is its build's folder, and where its archive and
/// its sum are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub name: String,
    pub archive: Url,
    pub sum: Url,
}

impl Release {
    /// The archive's one folder, and with `.tar.gz` its file name, which its sum names.
    fn top(&self) -> String {
        format!("{}-{BUILD}", self.name)
    }
}

/// `GE-Proton<major>-<minor>`'s two numbers, and none for any other name: a name is also a folder,
/// so nothing else is taken.
fn version(name: &str) -> Option<(u32, u32)> {
    let (major, minor) = name.strip_prefix("GE-Proton")?.split_once('-')?;
    let number = |part: &str| {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    Some((number(major)?, number(minor)?))
}

/// A build's version, from its folder's name.
fn build_version(home: &Path) -> Option<(u32, u32)> {
    version(home.file_name()?.to_str()?)
}

/// Whether `home` is a finished build: its `proton`, and its marker, written last.
fn finished(home: &Path) -> bool {
    home.join(MARKER).is_file() && home.join("proton").is_file()
}

/// The newest finished build in `dir`.
pub fn installed(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let home = entry.path();
            let v = build_version(&home)?;
            finished(&home).then_some((v, home))
        })
        .max_by_key(|(v, _)| *v)
        .map(|(_, home)| home)
}

/// An https address, or an http one on this machine (tests), and whether it's local.
fn checked(address: &str) -> Result<(Url, bool), ProtonError> {
    let url = Url::parse(address).map_err(|e| ProtonError::Download(e.to_string()))?;
    match url.scheme() {
        "https" => Ok((url, false)),
        "http" if crate::api::loopback(&url) => Ok((url, true)),
        _ => Err(ProtonError::Download(
            "its address must start with https://".into(),
        )),
    }
}

fn client(local: bool) -> Result<reqwest::Client, ProtonError> {
    crate::tools::client(local).map_err(|e| ProtonError::Download(e.to_string()))
}

fn failed(e: reqwest::Error) -> ProtonError {
    ProtonError::Download(e.without_url().to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `work`, or [`ProtonError::Cancelled`] as soon as `stop` says so, looked at every
/// [`LOOK_EVERY`]. Only work that holds nothing on disk is dropped this way.
async fn stoppable<T>(
    work: impl std::future::Future<Output = Result<T, ProtonError>>,
    stop: &Stop,
) -> Result<T, ProtonError> {
    let watch = async {
        while !stop() {
            tokio::time::sleep(LOOK_EVERY).await;
        }
    };
    tokio::select! {
        done = work => done,
        () = watch => Err(ProtonError::Cancelled),
    }
}

/// A small text from `address`: GitHub's API answer, or a sum file.
async fn small(address: &str) -> Result<String, ProtonError> {
    let (url, local) = checked(address)?;
    let mut res = client(local)?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(failed)?;
    if !res.status().is_success() {
        return Err(ProtonError::Download(format!(
            "the server answered {}",
            res.status()
        )));
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(failed)? {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > MAX_SMALL {
            return Err(ProtonError::TooLarge);
        }
    }
    String::from_utf8(body).map_err(|_| ProtonError::Release("it isn't text".into()))
}

/// GE-Proton's latest release, as `api` describes it.
pub async fn latest(api: &str) -> Result<Release, ProtonError> {
    let answer: serde_json::Value = serde_json::from_str(&small(api).await?)
        .map_err(|e| ProtonError::Release(e.to_string()))?;
    let name = answer["tag_name"]
        .as_str()
        .ok_or_else(|| ProtonError::Release("it names no release".into()))?;
    if version(name).is_none() {
        return Err(ProtonError::Release(format!("{name:?}")));
    }
    let asset = |file: &str| {
        answer["assets"]
            .as_array()?
            .iter()
            .find(|asset| asset["name"].as_str() == Some(file))?["browser_download_url"]
            .as_str()
            .map(str::to_owned)
    };
    let found = |file: String| {
        let address =
            asset(&file).ok_or_else(|| ProtonError::Release(format!("it has no {file}")))?;
        Ok::<Url, ProtonError>(checked(&address)?.0)
    };
    Ok(Release {
        archive: found(format!("{name}-{BUILD}.tar.gz"))?,
        sum: found(format!("{name}-{BUILD}.sha512sum"))?,
        name: name.to_owned(),
    })
}

/// The SHA-512 a release's sum file gives its archive `file`: `<128 hex digits>  <file>`.
fn expected(text: &str, file: &str) -> Result<[u8; 64], ProtonError> {
    let mut words = text.split_whitespace();
    let (Some(digits), Some(named)) = (words.next(), words.next()) else {
        return Err(ProtonError::Release("its sum file is empty".into()));
    };
    if named.trim_start_matches('*') != file {
        return Err(ProtonError::Release(format!("its sum is for {named}")));
    }
    let not_one = || ProtonError::Release("its sum isn't a SHA-512".into());
    if digits.len() != 128 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(not_one());
    }
    let mut sum = [0u8; 64];
    for (i, out) in sum.iter_mut().enumerate() {
        *out = u8::from_str_radix(&digits[2 * i..2 * i + 2], 16).map_err(|_| not_one())?;
    }
    Ok(sum)
}

/// Streams `url` into `part`, never past [`MAX_ARCHIVE`], and returns the SHA-512 of what came.
async fn fetch(
    url: &Url,
    part: &Path,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<[u8; 64], ProtonError> {
    let (url, local) = checked(url.as_str())?;
    let mut res = client(local)?.get(url).send().await.map_err(failed)?;
    if !res.status().is_success() {
        return Err(ProtonError::Download(format!(
            "the server answered {}",
            res.status()
        )));
    }
    let total = res.content_length();
    if total.is_some_and(|t| t > MAX_ARCHIVE) {
        return Err(ProtonError::TooLarge);
    }
    let mut out = std::fs::File::create(part)?;
    let mut sum = Sha512::new();
    let mut got = 0u64;
    while let Some(chunk) = res.chunk().await.map_err(failed)? {
        got += chunk.len() as u64;
        if got > MAX_ARCHIVE {
            return Err(ProtonError::TooLarge);
        }
        sum.update(&chunk);
        out.write_all(&chunk)?;
        progress(got, total);
    }
    out.sync_all()?;
    Ok(sum.finalize().into())
}

/// Unpacks `part`, a gzipped tar, into a fresh `new`: every entry under the archive's one folder
/// `top`, by plain names, a file, a folder or a symbolic link, at most [`MAX_UNPACKED`] in all,
/// with `top/proton` among them. `stop` is looked at before each entry.
fn unpack(
    part: &Path,
    new: &Path,
    top: &str,
    stop: &(dyn Fn() -> bool + Send + Sync),
) -> Result<(), ProtonError> {
    if new.exists() {
        std::fs::remove_dir_all(new)?;
    }
    std::fs::create_dir_all(new)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(part)?));
    tar.set_overwrite(false);
    let mut total = 0u64;
    for entry in tar.entries()? {
        if stop() {
            return Err(ProtonError::Cancelled);
        }
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let parts: Vec<Component> = path
            .components()
            .filter(|c| *c != Component::CurDir)
            .collect();
        let inside = parts.first() == Some(&Component::Normal(top.as_ref()))
            && parts.iter().all(|c| matches!(c, Component::Normal(_)));
        if !inside {
            return Err(ProtonError::Archive(format!(
                "{} isn't under {top}/",
                path.display()
            )));
        }
        use tar::EntryType;
        match entry.header().entry_type() {
            EntryType::Regular
            | EntryType::Continuous
            | EntryType::Directory
            | EntryType::Symlink => {}
            other => {
                return Err(ProtonError::Archive(format!(
                    "{} is a {other:?}",
                    path.display()
                )));
            }
        }
        total += entry.size();
        if total > MAX_UNPACKED {
            return Err(ProtonError::TooLarge);
        }
        if !entry.unpack_in(new)? {
            return Err(ProtonError::Archive(format!(
                "{} would land outside its folder",
                path.display()
            )));
        }
    }
    if !new.join(top).join("proton").is_file() {
        return Err(ProtonError::Archive(format!("it holds no {top}/proton")));
    }
    Ok(())
}

/// The release's build in `dir`: the one there when it's finished, or else fetched, checked against
/// the release's sum, unpacked and marked. `progress` hears the bytes so far and the total, when
/// known. Stopped or failed, it leaves nothing half-made behind.
pub async fn ensure(
    dir: &Path,
    release: &Release,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    stop: &Stop,
) -> Result<PathBuf, ProtonError> {
    if version(&release.name).is_none() {
        return Err(ProtonError::Release(format!("{:?}", release.name)));
    }
    let home = dir.join(&release.name);
    if finished(&home) {
        return Ok(home);
    }
    std::fs::create_dir_all(dir)?;
    sweep(dir);
    let part = dir.join(format!(".{}.part", release.name));
    let new = dir.join(format!(".{}.new", release.name));
    let result = async {
        let file = format!("{}.tar.gz", release.top());
        let sum = expected(&stoppable(small(release.sum.as_str()), stop).await?, &file)?;
        if stoppable(fetch(&release.archive, &part, progress), stop).await? != sum {
            return Err(ProtonError::Checksum);
        }
        let (from, into, top, stop) = (part.clone(), new.clone(), release.top(), stop.clone());
        tokio::task::spawn_blocking(move || unpack(&from, &into, &top, &*stop))
            .await
            .map_err(|e| ProtonError::Io(std::io::Error::other(e)))??;
        let built = new.join(release.top());
        std::fs::write(built.join(MARKER), hex(&sum))?;
        if home.exists() {
            std::fs::remove_dir_all(&home)?;
        }
        std::fs::rename(&built, &home)?;
        Ok(home.clone())
    }
    .await;
    let _ = std::fs::remove_file(&part);
    let _ = std::fs::remove_dir_all(&new);
    result
}

/// What fetches the launcher didn't live to finish left: `.<release>.part` and `.<release>.new`.
fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let left = name.to_str().and_then(|n| {
            let n = n.strip_prefix('.')?;
            n.strip_suffix(".part").or_else(|| n.strip_suffix(".new"))
        });
        if left.and_then(version).is_some() {
            remove(&entry);
        }
    }
}

/// Every other build in `dir`, finished or not, once `keep` is in place.
fn remove_older(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if build_version(&entry.path()).is_some() && entry.path() != keep {
            remove(&entry);
        }
    }
}

/// A folder with all it holds, or a file or link, never following a link.
fn remove(entry: &std::fs::DirEntry) {
    let path = entry.path();
    let gone = match entry.file_type() {
        Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
        _ => std::fs::remove_file(&path),
    };
    if let Err(e) = gone {
        warn!(error = %e, path = %path.display(), "an old GE-Proton file couldn't be removed");
    }
}

/// The GE-Proton Play hands umu-run: the latest, fetched into `dir` when it's newer than the build
/// there, else the build there, else none (umu-run's own default). `api` is GitHub's
/// ([`GE_RELEASES`]), or a test's. Once `stop` says so it returns soon, with the build there.
pub async fn for_play(
    dir: &Path,
    api: &str,
    progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    stop: &Stop,
) -> Option<PathBuf> {
    let have = installed(dir);
    let asked = stoppable(
        async {
            tokio::time::timeout(ASK_LIMIT, latest(api))
                .await
                .unwrap_or_else(|_| Err(ProtonError::Download("GitHub didn't answer".into())))
        },
        stop,
    )
    .await;
    let release = match asked {
        Ok(release) => release,
        Err(e) => {
            info!(error = %e, "GE-Proton's latest release couldn't be asked for");
            return have;
        }
    };
    if have.as_deref().and_then(build_version) >= version(&release.name) {
        return have;
    }
    match ensure(dir, &release, progress, stop).await {
        Ok(home) => {
            info!(release = %release.name, "fetched GE-Proton's latest release");
            remove_older(dir, &home);
            Some(home)
        }
        Err(ProtonError::Cancelled) => have,
        Err(e) => {
            warn!(error = %e, release = %release.name, "GE-Proton's latest release couldn't be fetched");
            have
        }
    }
}

/// Fakes for tests of GE-Proton's fetch, here and in Play's: archives shaped as GE-Proton's, and
/// a fake GitHub.
#[cfg(test)]
pub(crate) mod fakes {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A gzipped tar of `entries`: (path, body), a body of `None` being a symlink to `proton`. The
    /// names go into the headers as they are, `..` and all, as a hostile archive's would.
    pub(crate) fn targz(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.as_gnu_mut().unwrap().name[..path.len()].copy_from_slice(path.as_bytes());
            match body {
                Some(body) => {
                    header.set_size(body.len() as u64);
                    header.set_mode(0o755);
                    header.set_entry_type(tar::EntryType::Regular);
                    header.set_cksum();
                    tar.append(&header, *body).unwrap();
                }
                None => {
                    header.set_size(0);
                    header.set_mode(0o777);
                    header.set_entry_type(tar::EntryType::Symlink);
                    header.set_link_name("proton").unwrap();
                    header.set_cksum();
                    tar.append(&header, std::io::empty()).unwrap();
                }
            }
        }
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    pub(crate) fn sha512_hex(body: &[u8]) -> String {
        hex(&Sha512::digest(body))
    }

    /// A release's archive, shaped as GE-Proton's: one folder, `<tag>-x86_64/`.
    pub(crate) fn ge(tag: &str) -> Vec<u8> {
        let top = format!("{tag}-x86_64");
        targz(&[
            (&format!("{top}/proton"), Some(b"#!/usr/bin/env python3\n")),
            (&format!("{top}/files/bin/wine"), Some(b"\x7fELF")),
            (&format!("{top}/user_settings.sample.py"), Some(b"")),
            (&format!("{top}/toolmanifest.vdf"), Some(b"\"manifest\" {}")),
            (&format!("{top}/proton-link"), None),
        ])
    }

    /// A fake GitHub, and what was asked of it.
    pub(crate) struct Fake {
        pub(crate) api: String,
        asked: Arc<AtomicUsize>,
        hits: Arc<AtomicUsize>,
    }

    impl Fake {
        /// How many times its API was asked for the latest release.
        pub(crate) fn asked(&self) -> usize {
            self.asked.load(Ordering::SeqCst)
        }

        /// How many times its archive was fetched.
        pub(crate) fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    /// A fake GitHub: the API's latest release named `tag`, as GitHub's lists its assets, beside
    /// an aarch64 build; its archive `body`, at `archive` when given, and its sum file `sum`, or
    /// the archive's own.
    pub(crate) async fn github_at(
        tag: &str,
        body: Vec<u8>,
        sum: Option<String>,
        archive: Option<String>,
    ) -> Fake {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let archive = archive.unwrap_or_else(|| format!("{base}/archive"));
        let release = serde_json::json!({
            "tag_name": tag,
            "assets": [
                { "name": format!("{tag}-aarch64.sha512sum"), "browser_download_url": format!("{base}/other") },
                { "name": format!("{tag}-aarch64.tar.gz"), "browser_download_url": format!("{base}/other") },
                { "name": format!("{tag}-x86_64.sha512sum"), "browser_download_url": format!("{base}/sum") },
                { "name": format!("{tag}-x86_64.tar.gz"), "browser_download_url": archive },
            ],
        });
        let sum = sum.unwrap_or_else(|| format!("{}  {tag}-x86_64.tar.gz\n", sha512_hex(&body)));
        let (asked, hits) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (counted, asking) = (hits.clone(), asked.clone());
        let app = axum::Router::new()
            .route(
                "/api",
                axum::routing::get(move || {
                    asking.fetch_add(1, Ordering::SeqCst);
                    async move { axum::Json(release) }
                }),
            )
            .route(
                "/archive",
                axum::routing::get(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                    async move { body }
                }),
            )
            .route("/sum", axum::routing::get(move || async move { sum }));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Fake {
            api: format!("{base}/api"),
            asked,
            hits,
        }
    }

    pub(crate) async fn github(tag: &str, body: Vec<u8>, sum: Option<String>) -> Fake {
        github_at(tag, body, sum, None).await
    }

    /// An archive download that sends its first kilobyte of a megabyte, then nothing.
    pub(crate) async fn stalled() -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut request = [0u8; 4096];
                    let _ = sock.read(&mut request).await;
                    let head = "HTTP/1.1 200 OK\r\nContent-Length: 1048576\r\n\r\n";
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&[0u8; 1024]).await;
                    tokio::time::sleep(Duration::from_secs(600)).await;
                });
            }
        });
        format!("http://{addr}/archive")
    }

    /// A build already in `dir`, as `ensure` leaves one.
    pub(crate) fn built(dir: &Path, name: &str) -> PathBuf {
        let home = dir.join(name);
        std::fs::create_dir_all(home.join("files")).unwrap();
        std::fs::write(home.join("proton"), "#!/usr/bin/env python3\n").unwrap();
        std::fs::write(home.join(MARKER), "done").unwrap();
        home
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::fakes::*;
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn never() -> Stop {
        Arc::new(|| false)
    }

    /// An address nothing answers on: the connection is refused at once.
    const NOWHERE: &str = "http://127.0.0.1:1/api";

    #[tokio::test]
    async fn a_newer_release_is_fetched_checked_unpacked_and_the_older_one_removed() {
        let dir = tempfile::tempdir().unwrap();
        let old = built(dir.path(), "GE-Proton10-34");
        // What a launcher killed midway through earlier fetches left.
        std::fs::write(dir.path().join(".GE-Proton11-1.part"), "half").unwrap();
        std::fs::create_dir_all(dir.path().join(".GE-Proton11-1.new/x")).unwrap();
        let fake = github("GE-Proton11-9", ge("GE-Proton11-9"), None).await;
        let heard = std::sync::atomic::AtomicU64::new(0);
        let progress = |got: u64, _total: Option<u64>| heard.store(got, Ordering::Relaxed);
        let got = for_play(dir.path(), &fake.api, &progress, &never()).await;
        let home = dir.path().join("GE-Proton11-9");
        assert_eq!(got, Some(home.clone()));
        assert!(home.join("files/bin/wine").is_file());
        assert!(home.join(MARKER).is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.join("proton"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0, "proton keeps its executable bit");
            assert!(
                home.join("proton-link")
                    .symlink_metadata()
                    .unwrap()
                    .is_symlink()
            );
        }
        assert!(!old.exists(), "the older build is removed");
        assert!(heard.load(Ordering::Relaxed) > 0, "its progress is heard");
        assert_eq!(
            names(dir.path()),
            ["GE-Proton11-9"],
            "nothing half-made is left"
        );
        assert_eq!(fake.hits(), 1);
        // Asked again, the build it has is the latest: nothing more is fetched.
        assert_eq!(
            for_play(dir.path(), &fake.api, &|_, _| {}, &never()).await,
            Some(home)
        );
        assert_eq!(fake.hits(), 1);
    }

    #[tokio::test]
    async fn a_release_no_newer_than_the_build_it_has_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let have = built(dir.path(), "GE-Proton11-9");
        for tag in ["GE-Proton11-9", "GE-Proton11-8", "GE-Proton10-34"] {
            let fake = github(tag, ge(tag), None).await;
            let got = for_play(dir.path(), &fake.api, &|_, _| {}, &never()).await;
            assert_eq!(got, Some(have.clone()), "{tag}");
            assert_eq!(fake.hits(), 0, "{tag}: nothing is fetched");
        }
        assert_eq!(names(dir.path()), ["GE-Proton11-9"]);
    }

    #[tokio::test]
    async fn the_build_it_has_plays_when_github_doesnt_answer_and_none_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            for_play(dir.path(), NOWHERE, &|_, _| {}, &never()).await,
            None
        );
        let old = built(dir.path(), "GE-Proton10-34");
        assert_eq!(
            for_play(dir.path(), NOWHERE, &|_, _| {}, &never()).await,
            Some(old)
        );
    }

    #[tokio::test]
    async fn a_wrong_sum_is_refused_and_the_build_it_has_kept() {
        let dir = tempfile::tempdir().unwrap();
        let old = built(dir.path(), "GE-Proton10-34");
        let wrong = format!("{}  GE-Proton11-9-x86_64.tar.gz\n", "0".repeat(128));
        let fake = github("GE-Proton11-9", ge("GE-Proton11-9"), Some(wrong)).await;
        let got = for_play(dir.path(), &fake.api, &|_, _| {}, &never()).await;
        assert_eq!(got, Some(old.clone()));
        assert_eq!(
            names(dir.path()),
            ["GE-Proton10-34"],
            "nothing of it is kept"
        );
        // A sum for another file is no sum for this one.
        let body = ge("GE-Proton11-9");
        let other = format!("{}  GE-Proton11-9-aarch64.tar.gz\n", sha512_hex(&body));
        let fake = github("GE-Proton11-9", body, Some(other)).await;
        let got = for_play(dir.path(), &fake.api, &|_, _| {}, &never()).await;
        assert_eq!(got, Some(old));
        assert_eq!(fake.hits(), 0, "a sum that isn't its own fetches nothing");
    }

    #[tokio::test]
    async fn a_release_that_isnt_ge_protons_shape_is_refused_and_writes_nothing_outside() {
        let dir = tempfile::tempdir().unwrap();
        let protons = dir.path().join("proton");
        std::fs::create_dir_all(&protons).unwrap();
        // A name that isn't GE-Proton's.
        for tag in [
            "../evil",
            "GE-Proton11-9/../x",
            "Proton-9.0",
            "GE-Proton11",
            "GE-Proton11-9-x86_64",
        ] {
            let fake = github(tag, ge("GE-Proton11-9"), None).await;
            let got = for_play(&protons, &fake.api, &|_, _| {}, &never()).await;
            assert_eq!(got, None, "{tag}");
            assert_eq!(fake.hits(), 0, "{tag}");
        }
        // An archive whose entries aren't all under its own folder, or that holds no proton.
        for bad in [
            targz(&[("other/proton", Some(b"x"))]),
            targz(&[
                ("GE-Proton11-9-x86_64/proton", Some(b"x")),
                ("stray", Some(b"x")),
            ]),
            targz(&[
                ("GE-Proton11-9-x86_64/proton", Some(b"x")),
                ("../escape", Some(b"x")),
            ]),
            targz(&[("GE-Proton11-9-x86_64/../../escape", Some(b"x"))]),
            targz(&[("GE-Proton11-9-x86_64/files/x", Some(b"x"))]),
            b"not a gzip".to_vec(),
        ] {
            let fake = github("GE-Proton11-9", bad, None).await;
            assert_eq!(
                for_play(&protons, &fake.api, &|_, _| {}, &never()).await,
                None
            );
            assert_eq!(fake.hits(), 1);
        }
        assert!(!dir.path().join("escape").exists());
        assert!(names(&protons).is_empty(), "nothing half-made is left");
    }

    #[tokio::test]
    async fn a_stop_ends_a_stalled_download_at_once_and_leaves_nothing_half_made() {
        let dir = tempfile::tempdir().unwrap();
        let old = built(dir.path(), "GE-Proton10-34");
        let fake = github_at(
            "GE-Proton11-9",
            Vec::new(),
            Some(format!("{}  GE-Proton11-9-x86_64.tar.gz", "0".repeat(128))),
            Some(stalled().await),
        )
        .await;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop: Stop = {
            let stopped = stopped.clone();
            Arc::new(move || stopped.load(Ordering::SeqCst))
        };
        // Cancel start once its first kilobyte has come; nothing more ever does.
        let progress = |_: u64, _: Option<u64>| stopped.store(true, Ordering::SeqCst);
        let started = std::time::Instant::now();
        let got = for_play(dir.path(), &fake.api, &progress, &stop).await;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(stopped.load(Ordering::SeqCst), "the download had begun");
        assert_eq!(got, Some(old));
        assert_eq!(names(dir.path()), ["GE-Proton10-34"]);
    }

    #[test]
    fn an_unpack_looks_for_its_stop_before_each_entry() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join("part");
        std::fs::write(&part, ge("GE-Proton11-9")).unwrap();
        let new = dir.path().join("new");
        let looked = AtomicUsize::new(0);
        let stop = || looked.fetch_add(1, Ordering::SeqCst) >= 2;
        let unpacked = unpack(&part, &new, "GE-Proton11-9-x86_64", &stop);
        assert!(
            matches!(unpacked, Err(ProtonError::Cancelled)),
            "{unpacked:?}"
        );
        assert!(!new.join("GE-Proton11-9-x86_64/toolmanifest.vdf").exists());
        assert!(unpack(&part, &new, "GE-Proton11-9-x86_64", &|| false).is_ok());
    }

    #[test]
    fn the_newest_finished_build_is_the_one_it_has() {
        let dir = tempfile::tempdir().unwrap();
        built(dir.path(), "GE-Proton9-27");
        let newest = built(dir.path(), "GE-Proton10-3");
        // Unfinished: no marker.
        let unfinished = built(dir.path(), "GE-Proton11-1");
        std::fs::remove_file(unfinished.join(MARKER)).unwrap();
        std::fs::create_dir_all(dir.path().join("not-a-proton")).unwrap();
        assert_eq!(installed(dir.path()), Some(newest));
        assert_eq!(installed(&dir.path().join("none")), None);
    }

    /// GitHub's real latest release, fetched and unpacked as Play would: about 565 MB down and
    /// 1.6 GB on disk. Run by hand, with `--ignored`, when GE-Proton's releases may have changed
    /// shape.
    #[tokio::test]
    #[ignore = "fetches GE-Proton's latest release from GitHub"]
    async fn githubs_latest_release_is_fetched_and_unpacked() {
        let dir = tempfile::tempdir().unwrap();
        let release = latest(GE_RELEASES).await.unwrap();
        let home = for_play(dir.path(), GE_RELEASES, &|_, _| {}, &never()).await;
        assert_eq!(home, Some(dir.path().join(&release.name)));
        let home = home.unwrap();
        assert!(finished(&home));
        assert!(home.join("files/bin/wine").is_file());
        assert!(home.join("toolmanifest.vdf").is_file());
        assert_eq!(names(dir.path()), [release.name]);
    }
}
