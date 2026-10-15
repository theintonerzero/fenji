//! The model's sentences for what goes wrong (launcher spec §3.12), in DESIGN.md's voice: what
//! happened, and what to try. `server` is the server's name; `host` is its host's name, and
//! without one the words say "your host" ("Your host" at the start of a sentence).
use std::path::Path;

use crate::api::ApiError;
use crate::servers::AddError;

/// An invite code with no server saved to redeem it on (plan LP7).
pub const NEED_LINK: &str =
    "Paste the whole link your host sent: a code alone doesn't say which server it's for.";
/// Play on a system other than Linux (launcher spec L2).
pub const NOT_HERE: &str = "Fenji can't start the client on this system yet";
/// An account intent with no server in the list.
pub const NO_SERVER: &str = "Add a server first: paste the link or the address your host sent.";
/// A typed invite code that isn't one.
pub const BAD_CODE: &str = "That isn't an invite code. Check it against the link your host sent.";

/// A download off Linux (launcher spec L2).
pub const DOWNLOAD_LINUX_ONLY: &str = "Downloading with Steam works on Linux only for now.";
/// A check, download or repair while another runs.
pub const BUSY: &str = "Something is already running: wait for it to finish, or stop it.";
/// No terminal to open DepotDownloader in: the view shows `client.manualCommand` after it.
pub const NO_TERMINAL: &str =
    "Fenji couldn't open a terminal for DepotDownloader. Run this in yours:";
pub const CHECK_STOPPED: &str = "The check was stopped.";
pub const DOWNLOAD_CANCELLED: &str = "The download was cancelled.";
pub const WINDOW_CLOSED: &str =
    "DepotDownloader's window closed before the download finished. Try again.";
pub const NOTHING_TO_REPAIR: &str = "Every file checked out last time: there's nothing to repair.";
/// Play, a check or a repair with no game folder chosen (or one that's gone).
pub const NO_FOLDER: &str =
    "Fenji doesn't know where your game is yet. Set it up on the Client tab.";
/// Play while the game starts or runs (spec L12: one game at a time).
pub const RUNNING: &str = "The game is already running.";
/// Play when the quick check finds files to repair.
pub const NEEDS_REPAIR: &str = "Some game files need repair. The Client tab shows which.";
/// `umu-run createprefix` didn't make the Wine prefix.
pub const PREFIX_FAILED: &str =
    "Proton couldn't make the game's Wine prefix. The log folder has the details.";

/// The game ended with 0 within its first seconds: umu-run 1.4 logs a failure and ends that way
/// (#332).
pub const ENDED_AT_ONCE: &str =
    "The game couldn't start: umu-run ended at once. The log folder has the details.";

/// The game ended with another code than 0 within its first seconds.
pub fn couldnt_start(code: i32) -> String {
    format!("The game couldn't start: umu-run ended with {code}. The log folder has the details.")
}

/// The game ended with another code than 0 after it had run a while.
pub fn ended(code: i32) -> String {
    format!("The game ended with code {code}. The log folder has the details.")
}

/// umu-run couldn't be run at all.
pub fn couldnt_launch(why: &str) -> String {
    sentence("The game couldn't start", why)
}

/// A server that answered, but that this launcher can't play on (spec §3.12).
pub fn incompatible(server: &str, newer: bool) -> String {
    if newer {
        format!("{server} runs a newer Fenji. Update the launcher.")
    } else {
        format!("{server} expects a different build of the game than this launcher checks.")
    }
}

/// A game token that couldn't be taken, for another reason than the server's own answer.
pub fn no_ticket(server: &str, why: &str) -> String {
    sentence(&format!("Fenji couldn't get a ticket from {server}"), why)
}

/// DepotDownloader ended with another status than 0.
pub fn download_stopped(code: i32) -> String {
    format!("The download stopped: DepotDownloader ended with {code}. Try again.")
}

/// A tool couldn't be fetched (launcher plan Task 3: the tool's error doesn't name it).
pub fn tool_failed(tool: &str, why: &str) -> String {
    sentence(&format!("Fenji couldn't get {tool}"), why)
}

/// A check that failed for another reason than a missing folder or a stop.
pub fn check_failed(why: &str) -> String {
    sentence("Fenji couldn't check the files", why)
}

/// What an answer from the account API (or its failure) means to the player.
pub fn api_error(e: &ApiError, server: &str, host: Option<&str>, username: Option<&str>) -> String {
    let host_lower = host.unwrap_or("your host");
    let host_upper = host.unwrap_or("Your host");
    match e {
        ApiError::Http(_) => format!("Can't reach {server}."),
        ApiError::KeyChanged => {
            format!("{server}'s key has changed. Ask {host_lower} for a new link.")
        }
        ApiError::OwnKey => {
            format!("{server} uses its own key. Open the link {host_lower} sent you.")
        }
        ApiError::Maintenance { message, .. } => {
            sentence(&format!("{server} is in maintenance"), message)
        }
        ApiError::Banned { reason } => sentence(
            &format!("{host_upper} has banned this account from {server}"),
            reason,
        ),
        ApiError::Insecure => AddError::Insecure.to_string(),
        ApiError::BadUrl => AddError::Unreadable.to_string(),
        ApiError::Server {
            code,
            retry_after_secs,
            ..
        } => match code.as_str() {
            "bad_credentials" => {
                format!("That username and password don't match an account on {server}.")
            }
            "username_taken" => match username {
                Some(name) => format!("“{name}” is already taken on {server}. Try another name."),
                None => format!("That username is already taken on {server}. Try another name."),
            },
            "invalid_invite" => {
                format!("That invite has been used or has expired. Ask {host_lower} for a new one.")
            }
            "bad_username" => "Usernames are 3 to 16 letters, digits, _ or -.".into(),
            "bad_password" => "Passwords are 10 to 128 characters.".into(),
            "try_later" => match retry_after_secs {
                Some(n) => format!("Too many tries. Wait {n} s and try again."),
                None => "Too many tries. Wait a minute and try again.".into(),
            },
            "bad_session" => format!("Your session on {server} ended. Sign in again."),
            _ => unreadable(server),
        },
        ApiError::Decode(_) => unreadable(server),
    }
}

/// A redeem on a server whose key the player hasn't chosen yet.
pub fn key_first(server: &str) -> String {
    format!("Choose which key to trust for {server} first.")
}

/// The player took a new key for a server while signing in to it: the session from the old key
/// isn't kept.
pub fn key_changed_meanwhile(server: &str) -> String {
    format!("{server}'s key changed while you signed in. Sign in again.")
}

/// The settings couldn't be written.
pub fn cant_save(dir: &Path) -> String {
    format!(
        "Fenji couldn't save its settings in {}. Changes last until the launcher closes.",
        dir.display()
    )
}

fn unreadable(server: &str) -> String {
    format!("{server} gave an answer Fenji couldn't read.")
}

/// "lead: detail." — or "lead." with no detail. A detail that already ends a sentence keeps its
/// own stop, so the host's "Back around 21:00." doesn't gain a second.
fn sentence(lead: &str, detail: &str) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        format!("{lead}.")
    } else if detail.ends_with(['.', '!', '?']) {
        format!("{lead}: {detail}")
    } else {
        format!("{lead}: {detail}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_s_own_full_stop_is_kept_once() {
        let e = ApiError::Maintenance {
            message: "Back around 21:00.".into(),
            until: None,
        };
        assert_eq!(
            api_error(&e, "Moonward", None, None),
            "Moonward is in maintenance: Back around 21:00."
        );
        let e = ApiError::Maintenance {
            message: "  ".into(),
            until: None,
        };
        assert_eq!(
            api_error(&e, "Moonward", None, None),
            "Moonward is in maintenance."
        );
    }

    #[test]
    fn every_other_answer_has_words_too() {
        let say = |e: ApiError| api_error(&e, "Moonward", Some("Wen"), None);
        let code = |c: &str| ApiError::Server {
            status: 400,
            code: c.into(),
            retry_after_secs: None,
        };
        assert_eq!(
            say(ApiError::Http("refused".into())),
            "Can't reach Moonward."
        );
        assert_eq!(
            say(code("bad_credentials")),
            "That username and password don't match an account on Moonward."
        );
        assert_eq!(
            say(code("username_taken")),
            "That username is already taken on Moonward. Try another name."
        );
        assert_eq!(
            say(code("bad_username")),
            "Usernames are 3 to 16 letters, digits, _ or -."
        );
        assert_eq!(
            say(code("bad_password")),
            "Passwords are 10 to 128 characters."
        );
        assert_eq!(
            say(code("try_later")),
            "Too many tries. Wait a minute and try again."
        );
        assert_eq!(
            say(code("bad_session")),
            "Your session on Moonward ended. Sign in again."
        );
        assert_eq!(
            say(code("something_new")),
            "Moonward gave an answer Fenji couldn't read."
        );
        assert_eq!(
            say(ApiError::Decode("eh".into())),
            "Moonward gave an answer Fenji couldn't read."
        );
        assert_eq!(
            say(ApiError::Insecure),
            "Plain http:// works only for a server on this computer."
        );
    }
}
