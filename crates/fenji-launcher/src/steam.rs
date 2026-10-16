//! A copy of the game that Steam installed (launcher spec Amendment A, LA1 and LA2; L19). Steam's
//! own files say where it is: `steamapps/libraryfolders.vdf` lists the player's libraries, and a
//! library holding `steamapps/appmanifest_1418100.acf` holds the game, under the manifest's
//! `installdir`, at its `buildid`. They are read on this computer only. Fenji never writes into a
//! Steam library: Steam looks after what's there.
use std::path::{Component, Path, PathBuf};

use crate::settings::Os;

/// Swords of Legends Online's Steam app.
const APP: &str = "1418100";

/// The copy Steam's files describe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamCopy {
    /// `steamapps/common/<installdir>` in the library that holds it.
    pub folder: PathBuf,
    /// The manifest's `buildid`.
    pub build: String,
}

/// Steam's own folders, each with `steamapps/libraryfolders.vdf`: on Linux, the native
/// install's two names, the Flatpak's and the Snap's; on Windows, the folder the registry names
/// (`SteamPath`, read by the caller) and the default. None on macOS, where Steam can't install the
/// game.
pub fn roots(os: Os, home: Option<&Path>, registry: Option<PathBuf>) -> Vec<PathBuf> {
    match os {
        Os::Windows => registry
            .into_iter()
            .chain([PathBuf::from("C:/Program Files (x86)/Steam")])
            .collect(),
        Os::MacOs => Vec::new(),
        Os::Linux | Os::Other => home.map_or_else(Vec::new, |home| {
            [
                ".steam/steam",
                ".local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
                "snap/steam/common/.local/share/Steam",
            ]
            .iter()
            .map(|folder| home.join(folder))
            .collect()
        }),
    }
}

/// The game in the libraries Steam's folders `roots` list, if one holds it and its folder is
/// there.
pub fn find(roots: &[PathBuf]) -> Option<SteamCopy> {
    libraries(roots).into_iter().find_map(|library| {
        let apps = library.join("steamapps");
        let text = read_small(&apps.join(format!("appmanifest_{APP}.acf")))?;
        let top = parse(&text)?;
        let state = object(&top, "AppState")?;
        let installdir = string(state, "installdir")?;
        // A name, never a path: the manifest is a file anyone could have written. One plain
        // component: not ".", "..", a root or a drive (`C:` would replace the base on Windows).
        let mut parts = Path::new(installdir).components();
        let plain = matches!(
            (parts.next(), parts.next()),
            (Some(Component::Normal(_)), None)
        );
        if !plain || installdir.contains(['/', '\\']) {
            return None;
        }
        let folder = apps.join("common").join(installdir);
        folder.is_dir().then(|| SteamCopy {
            folder,
            build: string(state, "buildid").unwrap_or_default().to_string(),
        })
    })
}

/// Whether `folder` is inside a Steam library that `roots` list: Steam looks after it. It is, by
/// its path as written, by where that path leads, or inside where one of the library's game
/// folders links to, as when Steam's folder for a game is a link to another disk (#376). A folder
/// that isn't there yet is judged by where it would be.
pub fn in_library(folder: &Path, roots: &[PathBuf]) -> bool {
    let written = lexical(folder);
    let real = folder.ancestors().find_map(|existing| {
        Some(
            existing
                .canonicalize()
                .ok()?
                .join(folder.strip_prefix(existing).ok()?),
        )
    });
    libraries(roots).iter().any(|library| {
        let apps = library.join("steamapps");
        if written.starts_with(lexical(&apps)) {
            return true;
        }
        let Some(real) = &real else {
            return false;
        };
        apps.canonicalize().is_ok_and(|apps| real.starts_with(apps))
            || linked_games(&apps).iter().any(|game| real.starts_with(game))
    })
}

/// Where the game folders under `apps/common` that are links lead, resolved.
fn linked_games(apps: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(apps.join("common")) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_symlink()))
        .filter_map(|entry| entry.path().canonicalize().ok())
        .collect()
}

/// `path` with its `.` and `..` resolved by name, as it's written: no link is followed. A `..` at
/// the root stays there.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::ParentDir | Component::CurDir) | None => out.push(".."),
            },
            other => out.push(other),
        }
    }
    out
}

/// The folder the registry names for Steam: `HKCU\Software\Valve\Steam`'s `SteamPath`. Only on
/// Windows.
pub fn registry_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up from the launcher's window.
        // reg.exe by its full path: a bare "reg" is looked for in the launcher's own folder first.
        let windows = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let out = std::process::Command::new(windows.join(r"System32\reg.exe"))
            .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        steam_path(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(windows))]
    None
}

/// `SteamPath`'s value in `reg query`'s answer: `    SteamPath    REG_SZ    c:/program files (x86)/steam`.
#[cfg_attr(not(windows), allow(dead_code))]
fn steam_path(answer: &str) -> Option<PathBuf> {
    answer.lines().find_map(|line| {
        let (_, value) = line
            .trim()
            .strip_prefix("SteamPath")?
            .split_once("REG_SZ")?;
        let value = value.trim();
        (!value.is_empty()).then(|| PathBuf::from(value))
    })
}

/// Steam's own files are a few kilobytes. One larger than this is read as unreadable, never into
/// memory whole: these are files on the player's disk that anything could have written.
const MAX_FILE: u64 = 1 << 20;

fn read_small(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_FILE + 1)
        .read_to_string(&mut text)
        .ok()?;
    (text.len() as u64 <= MAX_FILE).then_some(text)
}

/// The libraries `roots`' `libraryfolders.vdf` list, each root's own first, each once.
fn libraries(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        let listed = read_small(&root.join("steamapps/libraryfolders.vdf"))
            .and_then(|text| parse(&text))
            .and_then(|top| object(&top, "libraryfolders").map(listed))
            .unwrap_or_default();
        for library in std::iter::once(root.clone()).chain(listed) {
            if library.join("steamapps").is_dir() && !found.contains(&library) {
                found.push(library);
            }
        }
    }
    found
}

/// The paths a `libraryfolders` object lists: each library's `path` since 2021, or before then,
/// a numbered key's value.
fn listed(libraries: &[(String, Kv)]) -> Vec<PathBuf> {
    libraries
        .iter()
        .filter(|(key, _)| key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|(_, node)| match node {
            Kv::Obj(fields) => string(fields, "path"),
            Kv::Str(path) => Some(path.as_str()),
        })
        .map(PathBuf::from)
        .collect()
}

/// A node of Valve's KeyValues text, as `.vdf` and `.acf` files hold it.
#[derive(Debug)]
enum Kv {
    Str(String),
    Obj(Vec<(String, Kv)>),
}

/// The object under `key`, Valve's keys being case-insensitive.
fn object<'a>(fields: &'a [(String, Kv)], key: &str) -> Option<&'a [(String, Kv)]> {
    fields.iter().find_map(|(k, node)| match node {
        Kv::Obj(inner) if k.eq_ignore_ascii_case(key) => Some(inner.as_slice()),
        _ => None,
    })
}

/// The string under `key`.
fn string<'a>(fields: &'a [(String, Kv)], key: &str) -> Option<&'a str> {
    fields.iter().find_map(|(k, node)| match node {
        Kv::Str(value) if k.eq_ignore_ascii_case(key) => Some(value.as_str()),
        _ => None,
    })
}

/// The whole text's keys and values, or none if it isn't KeyValues text: quoted strings with
/// their escapes, braces, and `//` comments, at most 32 objects deep.
fn parse(text: &str) -> Option<Vec<(String, Kv)>> {
    let mut tokens = tokens(text)?.into_iter().peekable();
    let fields = fields(&mut tokens, 0)?;
    tokens.next().is_none().then_some(fields)
}

#[derive(Debug, PartialEq)]
enum Token {
    Str(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => while chars.next_if(|&c| c != '\n').is_some() {},
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        },
                        other => s.push(other),
                    }
                }
                out.push(Token::Str(s));
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Key and value pairs up to a closing brace, or the end at the top.
fn fields(
    tokens: &mut std::iter::Peekable<std::vec::IntoIter<Token>>,
    depth: usize,
) -> Option<Vec<(String, Kv)>> {
    let mut out = Vec::new();
    loop {
        let key = match tokens.next() {
            None if depth == 0 => return Some(out),
            Some(Token::Close) if depth > 0 => return Some(out),
            Some(Token::Str(key)) => key,
            _ => return None,
        };
        let value = match tokens.next()? {
            Token::Str(value) => Kv::Str(value),
            Token::Open if depth < 32 => Kv::Obj(fields(tokens, depth + 1)?),
            _ => return None,
        };
        out.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Steam folder at `root`: its `libraryfolders.vdf` lists `libraries` (itself included as
    /// Steam lists it), and `holder`, if any, holds the game at `build`.
    fn steam(root: &Path, libraries: &[&Path], holder: Option<(&Path, &str)>) {
        let apps = root.join("steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        let mut vdf = String::from("\"libraryfolders\"\n{\n");
        for (n, lib) in libraries.iter().enumerate() {
            // Steam writes Windows paths with doubled backslashes; on Linux, as they are.
            let path = lib.display().to_string().replace('\\', "\\\\");
            vdf.push_str(&format!(
                "\t\"{n}\"\n\t{{\n\t\t\"path\"\t\t\"{path}\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{{\n\t\t}}\n\t}}\n"
            ));
        }
        vdf.push_str("}\n");
        std::fs::write(apps.join("libraryfolders.vdf"), vdf).unwrap();
        if let Some((lib, build)) = holder {
            let apps = lib.join("steamapps");
            std::fs::create_dir_all(apps.join("common/Swords of Legends Online/bin64")).unwrap();
            std::fs::write(
                apps.join("appmanifest_1418100.acf"),
                format!(
                    "\"AppState\"\n{{\n\t\"appid\"\t\t\"1418100\"\n\t\"name\"\t\t\"Swords of Legends Online\"\n\t\"installdir\"\t\t\"Swords of Legends Online\"\n\t\"buildid\"\t\t\"{build}\"\n}}\n"
                ),
            )
            .unwrap();
        }
    }

    #[test]
    fn the_game_is_found_in_whichever_library_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let (root, games) = (dir.path().join("Steam"), dir.path().join("Games"));
        steam(&root, &[&root, &games], Some((&games, "10015727")));
        let copy = find(std::slice::from_ref(&root)).unwrap();
        assert_eq!(
            copy,
            SteamCopy {
                folder: games.join("steamapps/common/Swords of Legends Online"),
                build: "10015727".into(),
            }
        );
        assert!(in_library(&copy.folder, std::slice::from_ref(&root)));
        assert!(!in_library(&dir.path().join("elsewhere"), &[root]));
    }

    #[test]
    fn libraries_steam_listed_before_2021_are_read_too() {
        // Before 2021, each library was a numbered key with its path as the value.
        let dir = tempfile::tempdir().unwrap();
        let (root, games) = (dir.path().join("Steam"), dir.path().join("Games"));
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"LibraryFolders\"\n{{\n\t\"TimeNextStatsReport\"\t\t\"1700000000\"\n\t\"ContentStatsID\"\t\t\"-42\"\n\t\"1\"\t\t\"{}\"\n}}\n",
                games.display()
            ),
        )
        .unwrap();
        steam(&games, &[], Some((&games, "10015727")));
        assert_eq!(
            find(&[root]).map(|c| c.folder),
            Some(games.join("steamapps/common/Swords of Legends Online"))
        );
    }

    #[test]
    fn nothing_is_found_without_the_game_or_its_folder_and_garbage_is_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        steam(&root, &[&root], None);
        assert_eq!(find(std::slice::from_ref(&root)), None);
        // A manifest whose folder is gone, as after a removal Steam didn't finish.
        steam(&root, &[&root], Some((&root, "10015727")));
        std::fs::remove_dir_all(root.join("steamapps/common")).unwrap();
        assert_eq!(find(std::slice::from_ref(&root)), None);
        std::fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            "\"libraryfolders\" { \"0\" {",
        )
        .unwrap();
        assert_eq!(find(std::slice::from_ref(&root)), None);
        assert_eq!(find(&[dir.path().join("no Steam here")]), None);
    }

    #[test]
    fn a_manifest_names_a_folder_never_a_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        steam(&root, &[&root], Some((&root, "10015727")));
        // Where "../../elsewhere" would land, from steamapps/common.
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        for installdir in ["../../elsewhere", "..", ".", "/etc", "a\\\\b", ""] {
            std::fs::write(
                root.join("steamapps/appmanifest_1418100.acf"),
                format!("\"AppState\" {{ \"installdir\" \"{installdir}\" \"buildid\" \"1\" }}"),
            )
            .unwrap();
            assert_eq!(find(std::slice::from_ref(&root)), None, "{installdir:?}");
        }
    }

    #[test]
    fn a_file_too_large_for_steam_to_have_written_is_unreadable() {
        // Steam's own files are a few kilobytes: a huge one is read as nothing, never whole.
        let dir = tempfile::tempdir().unwrap();
        let (root, games) = (dir.path().join("Steam"), dir.path().join("Games"));
        steam(&root, &[&root, &games], Some((&games, "10015727")));
        let vdf = root.join("steamapps/libraryfolders.vdf");
        let mut text = std::fs::read_to_string(&vdf).unwrap();
        text.push_str(&" ".repeat(2 << 20));
        std::fs::write(&vdf, text).unwrap();
        assert_eq!(find(std::slice::from_ref(&root)), None);
    }

    #[test]
    fn a_folder_not_there_yet_is_judged_by_where_it_would_be() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        steam(&root, &[&root], Some((&root, "10015727")));
        let roots = [root.clone()];
        assert!(in_library(
            &root.join("steamapps/common/A New Folder/deeper"),
            &roots
        ));
        assert!(!in_library(&dir.path().join("Games/SoLO"), &roots));
    }

    #[cfg(unix)]
    #[test]
    fn a_game_folder_that_links_to_another_disk_is_still_steams() {
        // #376 (the redesign's final review, Minor 6): Steam's folder for the game can be a link
        // to another disk. As it's written, it's in the library; resolved, it's elsewhere.
        let dir = tempfile::tempdir().unwrap();
        let (root, disk) = (dir.path().join("Steam"), dir.path().join("disk2/SoLO"));
        steam(&root, &[&root], Some((&root, "10015727")));
        let folder = root.join("steamapps/common/Swords of Legends Online");
        std::fs::remove_dir_all(&folder).unwrap();
        std::fs::create_dir_all(disk.join("bin64")).unwrap();
        std::os::unix::fs::symlink(&disk, &folder).unwrap();
        let roots = [root.clone()];
        let copy = find(&roots).unwrap();
        assert_eq!(copy.folder, folder);
        assert!(in_library(&copy.folder, &roots));
        assert!(in_library(
            &root.join("steamapps/common/./Swords of Legends Online/bin64/.."),
            &roots
        ));
        // The folder the link leads to is that copy too, chosen by its own path.
        assert!(in_library(&disk, &roots));
        assert!(in_library(&disk.join("bin64"), &roots));
        // Beside it on the other disk is the player's own.
        assert!(!in_library(&dir.path().join("disk2/Other"), &roots));
        // A name that only begins like the library's is another folder.
        assert!(!in_library(&dir.path().join("Steam2/steamapps"), &roots));
    }

    #[test]
    fn dots_in_a_path_are_resolved_by_name() {
        assert_eq!(lexical(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        assert_eq!(lexical(Path::new("/../a")), PathBuf::from("/a"));
        assert_eq!(lexical(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn the_registry_names_steams_folder() {
        let answer = "\r\nHKEY_CURRENT_USER\\Software\\Valve\\Steam\r\n    SteamPath    REG_SZ    c:/program files (x86)/steam\r\n\r\n";
        assert_eq!(
            steam_path(answer),
            Some(PathBuf::from("c:/program files (x86)/steam"))
        );
        assert_eq!(
            steam_path("ERROR: The system was unable to find the specified registry key"),
            None
        );
    }

    #[test]
    fn another_build_is_found_and_says_which() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        steam(&root, &[&root], Some((&root, "9999999")));
        assert_eq!(find(&[root]).unwrap().build, "9999999");
    }

    #[test]
    fn steam_lives_in_these_folders() {
        let home = Path::new("/home/p");
        assert_eq!(
            roots(Os::Linux, Some(home), None),
            [
                "/home/p/.steam/steam",
                "/home/p/.local/share/Steam",
                "/home/p/.var/app/com.valvesoftware.Steam/.local/share/Steam",
                "/home/p/snap/steam/common/.local/share/Steam",
            ]
            .map(PathBuf::from)
        );
        let registry = PathBuf::from("D:/Steam");
        assert_eq!(
            roots(Os::Windows, None, Some(registry.clone())),
            [registry, PathBuf::from("C:/Program Files (x86)/Steam")]
        );
        assert!(roots(Os::MacOs, Some(home), None).is_empty());
    }
}
