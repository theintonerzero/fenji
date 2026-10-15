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
