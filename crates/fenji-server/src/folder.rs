//! The data folder is the server's user's (M4 hosting spec §3.5; the hosting plan's final review,
//! Minor 1). On Linux and macOS, `run` and the admin commands refuse a data folder another user
//! owns, or one every user may write in, before they write anything there:
//! - the server's files there, its lock, its console socket, the database, the backups, the logs
//!   and its key, would otherwise be someone else's to replace or swap for links;
//! - and a command run as root, such as a `backup` typed with `sudo`, would leave files of root's
//!   that the server, run as its own user, then can't open.
//!
//! A folder its group may write in is allowed: with a user's own group and a umask of 002, as some
//! systems give, that's every folder its user makes. `init` writes the config alone, never the data
//! folder, so it isn't checked: the reference deployment runs it as root, writing to /etc/fenji.
//! Attaching a console isn't either: its commands run in the server's process, as its user.
use std::path::Path;

/// Refuses the data folder `data` if another user owns it, saying whom to run as, or if every user
/// may write in it, saying how to take that away. A folder that isn't there yet passes: the server
/// makes it, owner-only.
///
/// # Errors
/// Why the folder is refused, or why it can't be looked at.
#[cfg(unix)]
pub fn check(data: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    use anyhow::Context;

    let meta = match std::fs::metadata(data) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("can't look at the data folder {}", data.display()));
        }
    };
    let me = rustix::process::geteuid().as_raw();
    let shown = crate::lock::shown(data);
    match refusal(&shown, meta.uid(), meta.mode(), me, crate::unix::user_name) {
        Some(why) => anyhow::bail!(why),
        None => Ok(()),
    }
}

/// Elsewhere than Linux and macOS, every data folder passes.
///
/// # Errors
/// Never.
#[cfg(not(unix))]
pub fn check(_data: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Why the data folder `data`, owned by the user `owner` with `mode`, is refused to a process
/// running as the user `me`, if it is; `name` names users. Hidden characters in a name or the path
/// are shown as escapes, and the path in a command is quoted as the shell needs it.
#[cfg(unix)]
fn refusal(
    data: &Path,
    owner: u32,
    mode: u32,
    me: u32,
    name: impl Fn(u32) -> Option<String>,
) -> Option<String> {
    use crate::text::{escape_hidden, shell_word};
    let folder = escape_hidden(&data.display().to_string());
    if owner != me {
        let named = |uid: u32| name(uid).map(|n| escape_hidden(&n));
        let (owners, mine) = (named(owner), named(me));
        // sudo takes a user without a name as `#<uid>`.
        let sudo = owners.clone().unwrap_or_else(|| format!("'#{owner}'"));
        let owner = owners.unwrap_or_else(|| format!("user {owner}"));
        let me = mine.unwrap_or_else(|| format!("user {me}"));
        return Some(format!(
            "the data folder {folder} is {owner}'s, and this runs as {me}: run fenji-server as \
             {owner} (sudo -u {sudo} fenji-server …), so that the files it makes there stay the \
             server's"
        ));
    }
    if mode & 0o002 != 0 {
        let quoted = shell_word(&escape_hidden(&data.display().to_string()));
        return Some(format!(
            "every user may write in the data folder {folder}, and so replace the server's files \
             there: take that away with chmod o-w {quoted}"
        ));
    }
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The names a system with a `fenji` user and a `wen` user gives.
    fn names(uid: u32) -> Option<String> {
        match uid {
            0 => Some("root".into()),
            998 => Some("fenji".into()),
            1000 => Some("wen".into()),
            _ => None,
        }
    }

    const DATA: &str = "/var/lib/fenji";

    #[test]
    fn the_servers_own_folder_passes_whatever_its_group_may_do() {
        for mode in [0o40700, 0o40750, 0o40770, 0o40775, 0o42770] {
            assert_eq!(
                refusal(Path::new(DATA), 998, mode, 998, names),
                None,
                "{mode:o}"
            );
        }
        // Root running over root's own folder.
        assert_eq!(refusal(Path::new(DATA), 0, 0o40700, 0, names), None);
    }

    #[test]
    fn another_users_folder_is_refused_with_whom_to_run_as() {
        // A `backup` typed with sudo over the service's folder.
        let why = refusal(Path::new(DATA), 998, 0o40700, 0, names).expect("refused");
        assert_eq!(
            why,
            "the data folder /var/lib/fenji is fenji's, and this runs as root: run fenji-server \
             as fenji (sudo -u fenji fenji-server …), so that the files it makes there stay the \
             server's"
        );
        // A user the system doesn't name is named by number, as sudo takes one.
        let why = refusal(Path::new(DATA), 1234, 0o40700, 1000, names).expect("refused");
        assert!(
            why.contains("is user 1234's, and this runs as wen")
                && why.contains("(sudo -u '#1234' fenji-server …)"),
            "{why}"
        );
    }

    #[test]
    fn a_folder_every_user_may_write_in_is_refused_with_how_to_close_it() {
        for mode in [0o40777, 0o40707, 0o41777, 0o40702] {
            let why = refusal(Path::new(DATA), 998, mode, 998, names).expect("refused");
            assert_eq!(
                why,
                "every user may write in the data folder /var/lib/fenji, and so replace the \
                 server's files there: take that away with chmod o-w /var/lib/fenji",
                "{mode:o}"
            );
        }
    }

    #[test]
    fn a_path_in_the_hint_is_quoted_for_the_shell_where_it_needs_to_be() {
        let odd = Path::new("/home/wen/My Server/it's");
        let why = refusal(odd, 1000, 0o40777, 1000, names).expect("refused");
        assert!(
            why.ends_with(r"chmod o-w '/home/wen/My Server/it'\''s'"),
            "{why}"
        );
        // Hidden characters in the path are shown as escapes, not acted on by the terminal.
        let hidden = Path::new("/srv/\u{1b}[2J");
        let why = refusal(hidden, 1000, 0o40777, 1000, names).expect("refused");
        assert!(!why.contains('\u{1b}'), "{why:?}");
    }

    #[test]
    fn a_missing_folder_passes_and_the_server_makes_it() {
        let dir = tempfile::tempdir().unwrap();
        check(&dir.path().join("data")).unwrap();
        assert!(!dir.path().join("data").exists(), "the check makes nothing");
    }

    #[test]
    fn a_real_folder_every_user_may_write_in_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        check(dir.path()).unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o757)).unwrap();
        let e = format!("{:#}", check(dir.path()).unwrap_err());
        assert!(e.contains("chmod o-w"), "{e}");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
        check(dir.path()).unwrap();
    }
}
