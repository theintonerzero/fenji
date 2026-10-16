//! A guess at where this server is, for `[server] region` (M4 hosting spec Amendment C, HC2): the
//! country of this computer's timezone, through tzdata's own tables. Nothing asks the network: a
//! guess from the IP address would ask a third party, and the server never contacts anyone but its
//! players.
use std::path::Path;

/// The longest `[server] region`, in characters.
pub const REGION_MAX: usize = 32;

/// tzdata's tables are a few kilobytes. One larger than this is read as missing, never whole.
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
        let localtime = std::fs::read_link("/etc/localtime").ok();
        guess_from(
            tz.as_deref(),
            localtime.as_deref(),
            Path::new("/usr/share/zoneinfo"),
        )
    }
    #[cfg(not(unix))]
    None
}

/// [`guess`] from `tz` (`TZ`'s value, if set), `localtime` (where `/etc/localtime` links, if it
/// does) and `zoneinfo` (tzdata's folder).
fn guess_from(tz: Option<&str>, localtime: Option<&Path>, zoneinfo: &Path) -> Option<String> {
    let zone = match tz {
        Some(tz) => zone_of_tz(tz)?,
        None => zone_of_link(&localtime?.to_string_lossy())?,
    };
    let code = lookup(&read_small(&zoneinfo.join("zone.tab"))?, 2, &zone, 0)?;
    let name = lookup(&read_small(&zoneinfo.join("iso3166.tab"))?, 0, &code, 1)?;
    (name.chars().count() <= REGION_MAX && crate::text::plain(&name)).then_some(name)
}

/// The zone `TZ` names: `Australia/Sydney`, `:Australia/Sydney`, or a path into a zoneinfo folder.
/// A rule such as `AEST-10AEDT` names none.
fn zone_of_tz(tz: &str) -> Option<String> {
    let tz = tz.strip_prefix(':').unwrap_or(tz);
    if tz.starts_with('/') {
        return zone_of_link(tz);
    }
    tz.contains('/').then(|| tz.to_owned())
}

/// The zone a path into a zoneinfo folder names, as `/etc/localtime` links on Linux
/// (`../usr/share/zoneinfo/Australia/Melbourne`) and macOS (`/var/db/timezone/zoneinfo/…`), less a
/// `posix/` or `right/` variant's folder.
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
fn lookup(table: &str, key: usize, value: &str, want: usize) -> Option<String> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .find(|cols| cols.get(key) == Some(&value))
        .and_then(|cols| cols.get(want).map(|v| v.trim().to_owned()))
        .filter(|v| !v.is_empty())
}

fn read_small(path: &Path) -> Option<String> {
    use std::io::Read;
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
             AU\t-3749+14458\tAustralia/Melbourne\tVictoria\n\
             AU\t-3352+15113\tAustralia/Sydney\tNew South Wales (most areas)\n\
             DE\t+5230+01322\tEurope/Berlin\tmost of Germany\n\
             GB\t+513030-0000731\tEurope/London\n\
             XX\t+0000+00000\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("iso3166.tab"),
            "# ISO 3166 alpha-2 country codes\n#\n#country-\n#code\tname of country, territory, area, or subdivision\n\
             AU\tAustralia\nDE\tGermany\nGB\tBritain (UK)\nXX\tA name far too long for a region's thirty-two\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn the_zone_etc_localtime_links_to_names_its_country() {
        let z = zoneinfo();
        let link = |p: &str| guess_from(None, Some(Path::new(p)), z.path());
        assert_eq!(
            link("../usr/share/zoneinfo/Australia/Melbourne").as_deref(),
            Some("Australia")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Germany")
        );
        // macOS's link, and the posix and right variants' folders.
        assert_eq!(
            link("/var/db/timezone/zoneinfo/Australia/Sydney").as_deref(),
            Some("Australia")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/posix/Europe/London").as_deref(),
            Some("Britain (UK)")
        );
        assert_eq!(
            link("/usr/share/zoneinfo/right/Europe/Berlin").as_deref(),
            Some("Germany")
        );
    }

    #[test]
    fn tz_wins_over_the_link_in_any_of_its_forms() {
        let z = zoneinfo();
        let melbourne = Path::new("../usr/share/zoneinfo/Australia/Melbourne");
        let tz = |t: &str| guess_from(Some(t), Some(melbourne), z.path());
        assert_eq!(tz("Europe/Berlin").as_deref(), Some("Germany"));
        assert_eq!(tz(":Europe/Berlin").as_deref(), Some("Germany"));
        assert_eq!(
            tz("/usr/share/zoneinfo/Europe/Berlin").as_deref(),
            Some("Germany")
        );
        // A TZ that's a rule names no zone, and the guess doesn't fall back to the link: TZ is
        // what this computer's clock follows.
        assert_eq!(tz("AEST-10AEDT"), None);
    }

    #[test]
    fn a_zone_in_no_country_or_a_table_that_doesnt_say_gives_no_guess() {
        let z = zoneinfo();
        for link in [
            "/usr/share/zoneinfo/Etc/UTC",
            "/usr/share/zoneinfo/UTC",
            "/usr/share/zoneinfo/Mars/Olympus",
            // A row too short to name the zone's country, and one whose name is too long.
            "/usr/share/zoneinfo/",
        ] {
            assert_eq!(
                guess_from(None, Some(Path::new(link)), z.path()),
                None,
                "{link}"
            );
        }
        assert_eq!(guess_from(None, None, z.path()), None);
        assert_eq!(
            guess_from(Some("XX/Long"), None, z.path()),
            None,
            "a zone missing from zone.tab"
        );
        // A code whose name is longer than a region may be.
        std::fs::write(z.path().join("zone.tab"), "XX\t+0000+00000\tFar/Away\n").unwrap();
        assert_eq!(guess_from(Some("Far/Away"), None, z.path()), None);
        // A missing table.
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(guess_from(Some("Europe/Berlin"), None, empty.path()), None);
    }
}
