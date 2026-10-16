//! The AppImage's environment, kept from the programs the launcher starts (#329). The image's
//! AppRun and its GTK hook point `PATH`, `LD_LIBRARY_PATH`, `PYTHONHOME`, `XDG_DATA_DIRS` and more
//! into the image, and a child that inherits them loads the image's files instead of its own:
//! umu-run, a Python zipapp, couldn't start at all. The launcher's own process keeps them, since
//! WebKit's helper processes need them.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What the image's runtime says about itself.
const OWN: [&str; 4] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD"];

/// Set by the image's GTK hook whatever the player had, so never the player's own.
const FIXED: [&str; 1] = ["GTK_THEME"];

/// `vars` as they would be outside the AppImage their `APPDIR` names: without [`OWN`] and
/// [`FIXED`], and without the image's folders in any list of paths, where an empty element goes
/// too (AppRun leaves one, the current folder, where the value was empty). A variable left with
/// nothing goes. Without an `APPDIR`, or with one that isn't an absolute folder below the root,
/// `vars` stay as they are.
pub fn outside(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    let vars: Vec<_> = vars.into_iter().collect();
    let Some(image) = image(&vars) else {
        return vars;
    };
    vars.into_iter()
        .filter(|(k, _)| !OWN.iter().chain(&FIXED).any(|n| k == *n))
        .filter_map(|(k, v)| without(&image, v).map(|v| (k, v)))
        .collect()
}

fn image(vars: &[(OsString, OsString)]) -> Option<PathBuf> {
    let (_, dir) = vars.iter().find(|(k, _)| k == "APPDIR")?;
    let dir = Path::new(dir);
    (dir.is_absolute() && dir.parent().is_some()).then(|| dir.to_path_buf())
}

/// `value` less the paths in it that are in `image`, or `None` when nothing else is left. A value
/// with none stays as it is, byte for byte.
fn without(image: &Path, value: OsString) -> Option<OsString> {
    let paths: Vec<PathBuf> = std::env::split_paths(&value).collect();
    if !paths.iter().any(|p| p.starts_with(image)) {
        return Some(value);
    }
    let kept: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| !p.as_os_str().is_empty() && !p.starts_with(image))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

/// Inside an AppImage on Linux, opens `target`, a link or a folder, with `xdg-open` started with
/// [`outside`] `vars`, and says how that went; the opener plugin would pass the image's
/// environment on. Elsewhere `None`: the plugin opens it.
pub fn open(
    target: &OsStr,
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> Option<std::io::Result<()>> {
    let vars: Vec<_> = vars.into_iter().collect();
    if !cfg!(target_os = "linux") || image(&vars).is_none() {
        return None;
    }
    let spawned = Command::new("xdg-open")
        .arg(target)
        .env_clear()
        .envs(outside(vars))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    Some(spawned.map(|mut child| {
        // Reaped off this thread: xdg-open can wait for the program it hands the target to.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE: &str = "/tmp/.mount_FenjiAb12";

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v.replace("$IMG", IMAGE))))
            .collect()
    }

    #[test]
    fn outside_an_appimage_nothing_changes() {
        let v = vars(&[
            ("PATH", "/usr/local/bin:/usr/bin"),
            ("PYTHONPATH", "/opt/py:"),
            ("GTK_THEME", "Adwaita:dark"),
        ]);
        assert_eq!(outside(v.clone()), v);
    }

    /// What AppRun and the GTK hook set, from `strings AppRun.wrapped` and
    /// `apprun-hooks/linuxdeploy-plugin-gtk.sh` in the launcher's own AppImage.
    #[test]
    fn the_images_folders_and_its_own_variables_are_left_out() {
        let v = vars(&[
            ("APPDIR", "$IMG"),
            ("APPIMAGE", "/home/p/Fenji.AppImage"),
            ("ARGV0", "./Fenji.AppImage"),
            ("OWD", "/home/p"),
            ("HOME", "/home/p"),
            // A folder whose name only starts with the image's isn't in it.
            (
                "PATH",
                "$IMG/usr/bin/:$IMG/usr/sbin/:$IMG/bin/:/tmp/.mount_FenjiAb12x/bin:/usr/bin",
            ),
            ("PYTHONHOME", "$IMG/usr/"),
            // Built from an empty value, so they end in an empty element: the current folder.
            ("PYTHONPATH", "$IMG/usr/share/pyshared/:"),
            ("LD_LIBRARY_PATH", "$IMG/usr/lib/:$IMG/lib64/:"),
            (
                "XDG_DATA_DIRS",
                "$IMG/usr/share/:$IMG/usr/share:/usr/share:/usr/local/share",
            ),
            ("GTK_PATH", "$IMG//usr/lib/gtk-3.0"),
            ("GTK_DATA_PREFIX", "$IMG"),
            ("GTK_THEME", "Adwaita:dark"),
            // Not a list of paths, and nothing of the image's in it.
            ("LS_COLORS", "di=01;34:ln=01;36:"),
        ]);
        assert_eq!(
            outside(v),
            vars(&[
                ("HOME", "/home/p"),
                ("PATH", "/tmp/.mount_FenjiAb12x/bin:/usr/bin"),
                ("XDG_DATA_DIRS", "/usr/share:/usr/local/share"),
                ("LS_COLORS", "di=01;34:ln=01;36:"),
            ])
        );
    }

    #[test]
    fn an_appdir_with_a_trailing_slash_names_the_same_image() {
        let v = vars(&[("APPDIR", "$IMG/"), ("PATH", "$IMG/usr/bin:/usr/bin")]);
        assert_eq!(outside(v), vars(&[("PATH", "/usr/bin")]));
    }

    #[test]
    fn an_appdir_that_names_no_image_changes_nothing() {
        for appdir in ["/", "", "squashfs-root"] {
            let v = vars(&[
                ("APPDIR", appdir),
                ("PATH", "/usr/bin:squashfs-root/usr/bin"),
            ]);
            assert_eq!(outside(v.clone()), v, "{appdir:?}");
        }
    }

    #[test]
    fn outside_an_appimage_the_opener_plugin_opens_it() {
        assert!(
            open(
                OsStr::new("https://example.org"),
                vars(&[("PATH", "/usr/bin")])
            )
            .is_none()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn inside_one_xdg_open_gets_the_environment_outside_it() {
        use std::os::unix::fs::PermissionsExt;
        let spawn = crate::test_support::spawning();
        let dir = tempfile::tempdir().unwrap();
        let seen = dir.path().join("seen");
        let xdg_open = dir.path().join("xdg-open");
        std::fs::write(
            &xdg_open,
            format!(
                "#!/bin/sh\n{{ printf '%s\\n' \"$1\"; env | sort; }} > {0}.part && mv {0}.part {0}\n",
                seen.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&xdg_open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("$IMG/usr/bin:{}:/usr/bin:/bin", dir.path().display());
        let v = vars(&[
            ("APPDIR", "$IMG"),
            ("PATH", &path),
            ("PYTHONHOME", "$IMG/usr/"),
        ]);
        open(OsStr::new("https://example.org/a b"), v)
            .expect("inside an AppImage, it opens it itself")
            .unwrap();
        drop(spawn);
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !seen.exists() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(&seen).expect("xdg-open never ran");
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| !l.starts_with("PWD=") && !l.starts_with("SHLVL=") && !l.starts_with("_="))
            .collect();
        assert_eq!(
            lines,
            [
                "https://example.org/a b".to_string(),
                format!("PATH={}:/usr/bin:/bin", dir.path().display()),
            ]
        );
    }
}
