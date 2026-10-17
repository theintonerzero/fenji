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
/// [`FIXED`], and without the image's folders, or any other image's mount ([`in_a_mount`]), in
/// any list of paths, where an empty element goes too (AppRun leaves one, the current folder,
/// where the value was empty). A variable left with nothing goes. Without an `APPDIR`, or with one
/// that isn't an absolute folder below the root, `vars` stay as they are.
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

/// How the launcher starts itself again after an update inside an AppImage (#413): the image the
/// updater replaced, with the environment as it is outside the old image ([`outside`]), from the
/// folder the player started it in (the runtime's `OWD`, else their home). So the new launcher
/// loads none of the old image's libraries and doesn't hold its mount as its working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restart {
    pub program: PathBuf,
    pub env: Vec<(OsString, OsString)>,
    pub dir: PathBuf,
}

/// [`Restart`] for `vars`, or `None` outside an AppImage, or for an image named by a relative path:
/// Tauri's own restart, then.
pub fn restart(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Option<Restart> {
    let vars: Vec<_> = vars.into_iter().collect();
    image(&vars)?;
    let absolute = |name: &str| {
        vars.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| PathBuf::from(v))
            .filter(|p| p.is_absolute())
    };
    let program = absolute("APPIMAGE")?;
    let dir = absolute("OWD")
        .or_else(|| absolute("HOME"))
        .unwrap_or_else(|| PathBuf::from("/"));
    Some(Restart {
        program,
        env: outside(vars),
        dir,
    })
}

fn image(vars: &[(OsString, OsString)]) -> Option<PathBuf> {
    let (_, dir) = vars.iter().find(|(k, _)| k == "APPDIR")?;
    let dir = Path::new(dir);
    (dir.is_absolute() && dir.parent().is_some()).then(|| dir.to_path_buf())
}

/// Whether `path` is in an AppImage's mount: the folder an image's runtime makes in the
/// temporary folder, `.mount_` and a few letters of the image's name (`/tmp/.mount_Fenji_CAmBLK`),
/// or, for an image run with `--appimage-extract-and-run`, `appimage_extracted_` and a hash.
/// A launcher the updater restarted inherits the previous launcher's environment
/// (`tauri::process::restart`), so its variables hold the previous image's folders too, behind
/// its own; and that image stays mounted while the new launcher runs, since it holds the previous
/// runtime's keep-alive pipe. A child that inherited them loaded the previous image's files. An
/// extracted one's folder goes when the previous launcher ends, unless `NO_CLEANUP` keeps it.
fn in_a_mount(path: &Path) -> bool {
    path.ancestors().filter_map(Path::file_name).any(|name| {
        let name = name.as_encoded_bytes();
        name.starts_with(b".mount_") || name.starts_with(b"appimage_extracted_")
    })
}

/// `value` less the paths in it that are in `image` or in another image's mount, or `None` when
/// nothing else is left. A value with none stays as it is, byte for byte.
fn without(image: &Path, value: OsString) -> Option<OsString> {
    let paths: Vec<PathBuf> = std::env::split_paths(&value).collect();
    let imaged = |p: &PathBuf| p.starts_with(image) || in_a_mount(p);
    if !paths.iter().any(imaged) {
        return Some(value);
    }
    let kept: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| !p.as_os_str().is_empty() && !imaged(p))
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
    fn a_restart_runs_the_image_outside_its_own_environment_from_where_the_player_started_it() {
        // #413: the updater's restart passed on the old image's environment and folder, so the
        // new launcher loaded the old image's libraries and kept its mount busy.
        let old = "/tmp/.mount_Fenji_CAmBLK";
        let v = vars(&[
            ("APPDIR", "$IMG"),
            ("APPIMAGE", "/home/p/Fenji.AppImage"),
            ("OWD", "/home/p/Downloads"),
            (
                "LD_LIBRARY_PATH",
                &format!("$IMG/usr/lib/:{old}/usr/lib/:/opt/lib"),
            ),
            ("PWD", "$IMG/usr"),
            ("HOME", "/home/p"),
        ]);
        let plan = restart(v).unwrap();
        assert_eq!(plan.program, PathBuf::from("/home/p/Fenji.AppImage"));
        assert_eq!(plan.dir, PathBuf::from("/home/p/Downloads"));
        assert_eq!(
            plan.env,
            vars(&[("LD_LIBRARY_PATH", "/opt/lib"), ("HOME", "/home/p")])
        );
        // No folder the player started it in: their home, not the image's.
        let v = vars(&[
            ("APPDIR", "$IMG"),
            ("APPIMAGE", "/x/F.AppImage"),
            ("HOME", "/home/p"),
        ]);
        assert_eq!(restart(v).unwrap().dir, PathBuf::from("/home/p"));
        // Not in an image: Tauri's own restart.
        assert!(restart(vars(&[("HOME", "/home/p")])).is_none());
        let v = vars(&[("APPDIR", "$IMG"), ("APPIMAGE", "relative.AppImage")]);
        assert!(restart(v).is_none(), "an image named by a relative path");
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
            // Another image's mount, whose name only starts with this one's, goes too.
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
                ("PATH", "/usr/bin"),
                ("XDG_DATA_DIRS", "/usr/share:/usr/local/share"),
                ("LS_COLORS", "di=01;34:ln=01;36:"),
            ])
        );
    }

    #[test]
    fn a_launcher_the_updater_restarted_leaves_out_the_previous_images_folders_too() {
        // The updater restarts the launcher with its own environment (`tauri::process::restart`),
        // so the new image's AppRun puts its folders in front of the previous image's, which stays
        // mounted while the new launcher runs: umu-run then loaded the previous image's libcrypto,
        // older than the system's Python needs, and the game couldn't start (v0.5.3).
        let old = "/tmp/.mount_Fenji_CAmBLK";
        let v = vars(&[
            ("APPDIR", "$IMG"),
            (
                "LD_LIBRARY_PATH",
                &format!("$IMG/usr/lib/:$IMG/lib64/:{old}/usr/lib/:{old}/lib64/:/opt/lib"),
            ),
            ("PATH", &format!("$IMG/usr/bin/:{old}/usr/bin/:/usr/bin")),
            (
                "PYTHONPATH",
                &format!("$IMG/usr/share/pyshared/:{old}/usr/share/pyshared/:"),
            ),
            // The previous AppRun's folder, which the restart kept (#407's review).
            ("PWD", &format!("{old}/usr")),
        ]);
        assert_eq!(
            outside(v),
            vars(&[("LD_LIBRARY_PATH", "/opt/lib"), ("PATH", "/usr/bin")])
        );
    }

    #[test]
    fn a_launcher_the_updater_restarted_leaves_out_an_extracted_previous_image_too() {
        // Run with `--appimage-extract-and-run` and kept with `NO_CLEANUP`, the previous image's
        // folder outlives its launcher.
        let old = "/tmp/appimage_extracted_0f3c9a1d2b4e";
        let v = vars(&[
            ("APPDIR", "$IMG"),
            (
                "LD_LIBRARY_PATH",
                &format!("$IMG/usr/lib/:{old}/usr/lib/:/opt/lib"),
            ),
        ]);
        assert_eq!(outside(v), vars(&[("LD_LIBRARY_PATH", "/opt/lib")]));
    }

    #[test]
    fn a_folder_whose_name_only_starts_with_an_extracted_images_isnt_in_it() {
        // An AppDir extracted by hand (`--appimage-extract`) is wherever the player put it.
        let v = vars(&[
            ("APPDIR", "/opt/Fenji.AppDir"),
            (
                "PATH",
                "/opt/Fenji.AppDir/usr/bin:/opt/Fenji.AppDirX/bin:/usr/bin",
            ),
            ("XDG_DATA_DIRS", "/usr/share/.mounted:/usr/share"),
        ]);
        assert_eq!(
            outside(v),
            vars(&[
                ("PATH", "/opt/Fenji.AppDirX/bin:/usr/bin"),
                ("XDG_DATA_DIRS", "/usr/share/.mounted:/usr/share"),
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
