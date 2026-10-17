//! A guess at where this server is, for `[server] region` (M4 hosting spec Amendment C, HC2): the
//! country of this computer's timezone, through tzdata's own tables. Nothing asks the network: a
//! guess from the IP address would ask a third party, and the server never contacts anyone but its
//! players.
#[cfg(any(unix, test))]
use std::path::{Path, PathBuf};

/// The longest `[server] region`, in characters.
pub const REGION_MAX: usize = 32;

/// tzdata's tables are a few kilobytes. One larger than this is read as missing, never whole.
#[cfg(any(unix, test))]
const MAX_TABLE: u64 = 1 << 20;

/// The English name of the country this computer's timezone is in, as tzdata names it: `TZ`'s
/// zone, else the one `/etc/localtime` links to, through `/usr/share/zoneinfo`'s `zone.tab` (a
/// zone's country code) and `iso3166.tab` (a code's name). None on Windows, whose zone names
/// differ; for a zone in no country, such as `Etc/UTC`; and when a table is missing or doesn't
/// say.
pub fn guess() -> Option<String> {
    #[cfg(unix)]
    {
        let tz = std::env::var("TZ").ok();
        guess_from(
            tz.as_deref(),
            |path| std::fs::read_link(path).ok(),
            Path::new("/usr/share/zoneinfo"),
        )
    }
    #[cfg(not(unix))]
    None
}

/// [`guess`] from `tz` (`TZ`'s value, if set), `link` (where a file links, if it's a link) and
/// `zoneinfo` (tzdata's folder).
#[cfg(any(unix, test))]
fn guess_from(
    tz: Option<&str>,
    link: impl Fn(&Path) -> Option<PathBuf>,
    zoneinfo: &Path,
) -> Option<String> {
    let linked = |path: &str| zone_of_link(&link(Path::new(path))?.to_string_lossy());
    let zone = match tz {
        Some(tz) => zone_of_tz(tz, linked)?,
        None => linked("/etc/localtime")?,
    };
    let code = lookup(&read_small(&zoneinfo.join("zone.tab"))?, 2, &zone, 0)?;
    let name = lookup(&read_small(&zoneinfo.join("iso3166.tab"))?, 0, &code, 1)?;
    (name.chars().count() <= REGION_MAX && crate::text::plain(&name)).then_some(name)
}

/// The zone `TZ` names: `Europe/Berlin`, `:Europe/Berlin`, a path into a zoneinfo folder, or a
/// path to a file that links into one (`linked`), as `:/etc/localtime` does. A rule such as
/// `CET-1CEST` names none, and so does an empty `TZ`, which the C library reads as UTC.
#[cfg(any(unix, test))]
fn zone_of_tz(tz: &str, linked: impl Fn(&str) -> Option<String>) -> Option<String> {
    let tz = tz.strip_prefix(':').unwrap_or(tz);
    if tz.starts_with('/') {
        return zone_of_link(tz).or_else(|| linked(tz));
    }
    tz.contains('/').then(|| tz.to_owned())
}

/// The zone a path into a zoneinfo folder names, as `/etc/localtime` links on Linux
/// (`../usr/share/zoneinfo/Europe/Berlin`) and macOS (`/var/db/timezone/zoneinfo/…`), less a
/// `posix/` or `right/` variant's folder.
#[cfg(any(unix, test))]
fn zone_of_link(path: &str) -> Option<String> {
    let (_, zone) = path.rsplit_once("zoneinfo/")?;
    let zone = zone
        .strip_prefix("posix/")
        .or_else(|| zone.strip_prefix("right/"))
        .unwrap_or(zone);
    (!zone.is_empty()).then(|| zone.to_owned())
}

/// The `want` column of the first row whose `key` column is `value`, in a tzdata table: tab-
/// separated, with `#` comments. Rows too short for either column are skipped.
#[cfg(any(unix, test))]
fn lookup(table: &str, key: usize, value: &str, want: usize) -> Option<String> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .find(|cols| cols.get(key) == Some(&value))
        .and_then(|cols| cols.get(want).map(|v| v.trim().to_owned()))
        .filter(|v| !v.is_empty())
}

/// A table's text, if it's a file of at most [`MAX_TABLE`] bytes. Anything else, such as a FIFO
/// or a device, which a read could wait on forever, is read as missing.
#[cfg(any(unix, test))]
fn read_small(path: &Path) -> Option<String> {
    use std::io::Read;
    if !std::fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_TABLE + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_TABLE).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zoneinfo folder holding tzdata's two tables, cut down to a few rows, with their comments.
    fn zoneinfo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("zone.tab"),
            "# tzdb timezone descriptions\n#\n# country-\n#code\tcoordinates\tTZ\tcomments\n\
             CA\t+4339-07923\tAmerica/Toronto\tEastern - ON & QC (most areas)\n\
             CA\t+4916-12307\tAmerica/Vancouver\tPacific - BC (most areas)\n\
             DE\t+5230+01322\tEurope/Berlin\tmost of Germany\n\
             GB\t+513030-0000731\tEurope/London\n\
             XX\t+0000+00000\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("iso3166.tab"),
            "# ISO 3166 alpha-2 country codes\n#\n#country-\n#code\tname of country, territory, area, or subdivision\n\
             CA\tCanada\nDE\tGermany\nGB\tBritain (UK)\nXX\tA name far too long for a region's thirty-two\n",
        )
        .unwrap();
        dir
    }

    /// A computer whose `/etc/localtime` links to `target`, and nothing else is a link.
    fn localtime(target: &str) -> impl Fn(&Path) -> Option<PathBuf> + '_ {
        move |path| (path == Path::new("/etc/localtime")).then(|| PathBuf::from(target))
    }

    /// No links at all.
    fn no_links(_: &Path) -> Option<PathBuf> {
        None
    }

    #[test]
    fn the_zone_etc_localtime_links_to_names_its_country() {
        let z = zoneinfo();
        let link = |target: &str| guess_from(None, localtime(target), z.path());
        assert_eq!(
            link("../usr/share/zoneinfo/America/Toronto").as_deref(),
            Some("Canada")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Germany")
        );
        // macOS's link, and the posix and right variants' folders.
        assert_eq!(
            link("/var/db/timezone/zoneinfo/America/Vancouver").as_deref(),
            Some("Canada")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/posix/Europe/London").as_deref(),
            Some("Britain (UK)")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/right/Europe/Berlin").as_deref(),
            Some("Germany")
        );
        assert_eq!(guess_from(None, no_links, z.path()), None, "no link");
    }

    #[test]
    fn tz_wins_over_the_link_in_any_of_its_forms() {
        let z = zoneinfo();
        let toronto = "../usr/share/zoneinfo/America/Toronto";
        let tz = |t: &str| guess_from(Some(t), localtime(toronto), z.path());
        assert_eq!(tz("Europe/Berlin").as_deref(), Some("Germany"));
        assert_eq!(tz(":Europe/Berlin").as_deref(), Some("Germany"));
        assert_eq!(
            tz("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Germany")
        );
        // A TZ that names /etc/localtime itself is followed as the link is.
        assert_eq!(tz(":/etc/localtime").as_deref(), Some("Canada"));
        assert_eq!(tz("/etc/localtime").as_deref(), Some("Canada"));
        // A TZ that's a rule names no zone, nor does an empty one, which the C library reads as
        // UTC, and the guess doesn't fall back to the link: TZ is what this computer's clock
        // follows.
        assert_eq!(tz("CET-1CEST"), None);
        assert_eq!(tz(""), None);
        assert_eq!(tz(":"), None);
    }

    #[test]
    fn a_zone_in_no_country_or_a_table_that_doesnt_say_gives_no_guess() {
        let z = zoneinfo();
        for target in [
            "/usr/share/zoneinfo/Etc/UTC",
            "/usr/share/zoneinfo/UTC",
            "/usr/share/zoneinfo/Mars/Olympus",
            // A row too short to name the zone's country, and one whose name is too long.
            "/usr/share/zoneinfo/",
        ] {
            assert_eq!(
                guess_from(None, localtime(target), z.path()),
                None,
                "{target}"
            );
        }
        assert_eq!(
            guess_from(Some("XX/Long"), no_links, z.path()),
            None,
            "a zone missing from zone.tab"
        );
        // A code whose name is longer than a region may be.
        std::fs::write(z.path().join("zone.tab"), "XX\t+0000+00000\tFar/Away\n").unwrap();
        assert_eq!(guess_from(Some("Far/Away"), no_links, z.path()), None);
        // A name that isn't plain text, which would end up in the first run's own config.
        std::fs::write(z.path().join("zone.tab"), "YY\t+0000+00000\tOdd/Place\n").unwrap();
        std::fs::write(z.path().join("iso3166.tab"), "YY\tOdd\u{7}land\n").unwrap();
        assert_eq!(guess_from(Some("Odd/Place"), no_links, z.path()), None);
        // A missing table.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            guess_from(Some("Europe/Berlin"), no_links, empty.path()),
            None
        );
    }

    #[test]
    fn a_table_too_large_or_not_a_file_is_read_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zone.tab");
        std::fs::write(&path, vec![b'#'; MAX_TABLE as usize]).unwrap();
        assert!(read_small(&path).is_some(), "at the limit");
        std::fs::write(&path, vec![b'#'; MAX_TABLE as usize + 1]).unwrap();
        assert_eq!(read_small(&path), None, "one byte over");
        assert_eq!(read_small(dir.path()), None, "a folder");
        #[cfg(unix)]
        {
            // A FIFO would wait for a writer that never comes. The read runs on a thread of its
            // own, so that one that waits fails this test rather than hanging it.
            let fifo = dir.path().join("fifo.tab");
            let made = std::process::Command::new("mkfifo").arg(&fifo).status();
            if made.is_ok_and(|s| s.success()) {
                let (tx, rx) = std::sync::mpsc::channel();
                let path = fifo.clone();
                std::thread::spawn(move || tx.send(read_small(&path)));
                let read = rx.recv_timeout(std::time::Duration::from_secs(5));
                if read.is_err() {
                    // Lets the waiting read go before failing.
                    let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
                }
                assert_eq!(read, Ok(None), "a FIFO is read as missing, at once");
            }
        }
    }
}
