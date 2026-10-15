//! What changes about the server while it runs (M4 hosting spec §3.8, §3.9; §8, HA1): maintenance
//! mode, the MOTD and the names of the characters in the world. Maintenance and an MOTD set over
//! the file's are kept in the store's `server_state`, so they outlive a restart: each change is
//! stored first, then published on a `watch` channel, which the account API reads and the world
//! and the console follow.
use std::sync::Arc;

use anyhow::bail;
use fenji_store::Store;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, watch};

use crate::commands::{self, CommandError};

/// The most characters a maintenance message, or an MOTD, may have.
pub const MESSAGE_MAX: usize = 200;

/// The message of a maintenance mode turned on without one. Players read it after words that say
/// the server is in maintenance already, such as the launcher's "Moonward is in maintenance: ", so
/// it says what comes next.
pub const DEFAULT_MESSAGE: &str = "Back soon";

/// `server_state`'s key for maintenance mode, as JSON, while it's on.
const MAINTENANCE_KEY: &str = "maintenance";
/// `server_state`'s key for the MOTD the host set over `[server] motd`.
const MOTD_KEY: &str = "motd";

/// Maintenance mode while it's on (spec §3.9): what players are told, since when, and when the host
/// expects to be back, which is shown to players and doesn't end maintenance by itself. Times are
/// Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Maintenance {
    pub message: String,
    pub since_ms: i64,
    pub until_ms: Option<i64>,
}

/// Checks a maintenance message: plain text ([`crate::text::plain`]) of at most [`MESSAGE_MAX`]
/// characters. It reaches every player's launcher, and the world's announcement. An MOTD is held
/// to the same.
pub fn check_message(text: &str) -> Result<(), CommandError> {
    if !crate::text::plain(text) {
        return Err(CommandError::NotPlain);
    }
    if text.chars().count() > MESSAGE_MAX {
        return Err(CommandError::Usage(commands::MAINTENANCE));
    }
    Ok(())
}

/// The server's changing facts, shared as `Arc<Status>`.
pub struct Status {
    started_ms: i64,
    /// `[server] motd`, which is the MOTD while the store holds none.
    file_motd: String,
    maintenance: watch::Sender<Option<Maintenance>>,
    /// The MOTD in effect: the store's when it has one, else the file's.
    motd: watch::Sender<String>,
    /// The characters in the world, sorted, as the world last sent them.
    names: watch::Sender<Arc<[String]>>,
    /// Held while a change is stored and published, so that the store and the channels agree.
    writing: Mutex<()>,
}

impl Status {
    /// The state `store` keeps, for a server that started at `started_ms` (Unix ms) with
    /// `file_motd` as its `[server] motd`. A stored maintenance mode this version can't read keeps
    /// the server in maintenance, with the default message, since `started_ms`: the host meant it
    /// shut, and can turn it off from the console.
    pub async fn load(
        store: &Store,
        started_ms: i64,
        file_motd: &str,
    ) -> anyhow::Result<Arc<Status>> {
        let maintenance = store.state(MAINTENANCE_KEY).await?.map(|json| {
            serde_json::from_str(&json).unwrap_or_else(|e| {
                tracing::warn!(
                    error = %e,
                    "the stored maintenance mode can't be read: the server stays in maintenance"
                );
                Maintenance {
                    message: DEFAULT_MESSAGE.into(),
                    since_ms: started_ms,
                    until_ms: None,
                }
            })
        });
        let motd = store
            .state(MOTD_KEY)
            .await?
            .unwrap_or_else(|| file_motd.to_owned());
        Ok(Arc::new(Status {
            started_ms,
            file_motd: file_motd.to_owned(),
            maintenance: watch::Sender::new(maintenance),
            motd: watch::Sender::new(motd),
            names: watch::Sender::new(Arc::from(Vec::new())),
            writing: Mutex::new(()),
        }))
    }

    /// When the server started, in Unix milliseconds.
    pub fn started_ms(&self) -> i64 {
        self.started_ms
    }

    pub fn maintenance(&self) -> Option<Maintenance> {
        self.maintenance.borrow().clone()
    }

    /// Turns maintenance on (`Some`) or off (`None`): stores it, then publishes it. A message that
    /// [`check_message`] refuses changes nothing.
    pub async fn set_maintenance(
        &self,
        store: &Store,
        m: Option<Maintenance>,
    ) -> anyhow::Result<()> {
        if let Some(m) = &m
            && check_message(&m.message).is_err()
        {
            bail!("a maintenance message is plain text of at most {MESSAGE_MAX} characters");
        }
        let _writing = self.writing.lock().await;
        match &m {
            Some(m) => {
                store
                    .set_state(MAINTENANCE_KEY, &serde_json::to_string(m)?)
                    .await?
            }
            None => store.clear_state(MAINTENANCE_KEY).await?,
        }
        self.maintenance.send_replace(m);
        Ok(())
    }

    /// Maintenance mode as it changes.
    pub fn watch_maintenance(&self) -> watch::Receiver<Option<Maintenance>> {
        self.maintenance.subscribe()
    }

    /// The MOTD in effect; empty for none.
    pub fn motd(&self) -> String {
        self.motd.borrow().clone()
    }

    /// Sets the MOTD over the file's and keeps it in the store (`Some`; an empty one is none), or
    /// drops the store's (`None`), which gives the file's back; then publishes the MOTD in effect.
    /// Text that [`check_message`] refuses changes nothing.
    pub async fn set_motd(&self, store: &Store, motd: Option<String>) -> anyhow::Result<()> {
        if let Some(text) = &motd
            && check_message(text).is_err()
        {
            bail!("an MOTD is plain text of at most {MESSAGE_MAX} characters");
        }
        let _writing = self.writing.lock().await;
        match &motd {
            Some(text) => store.set_state(MOTD_KEY, text).await?,
            None => store.clear_state(MOTD_KEY).await?,
        }
        self.motd
            .send_replace(motd.unwrap_or_else(|| self.file_motd.clone()));
        Ok(())
    }

    /// The MOTD in effect as it changes.
    pub fn watch_motd(&self) -> watch::Receiver<String> {
        self.motd.subscribe()
    }

    /// The characters in the world, sorted.
    pub fn names(&self) -> Arc<[String]> {
        Arc::clone(&self.names.borrow())
    }

    /// Where the world sends the sorted names of its characters when one enters or leaves. Until
    /// it does, there are none.
    pub fn names_sender(&self) -> watch::Sender<Arc<[String]>> {
        self.names.clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fenji_store::Store;

    use super::*;
    use crate::commands::{self, CommandError};

    const NOW: i64 = 1_800_000_000_000;

    async fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("fenji.db")).await.unwrap();
        (dir, store)
    }

    fn updating(until_ms: Option<i64>) -> Maintenance {
        Maintenance {
            message: "Updating".into(),
            since_ms: NOW,
            until_ms,
        }
    }

    #[test]
    fn a_message_is_plain_text_of_200_characters_at_most() {
        assert_eq!(MESSAGE_MAX, 200);
        assert_eq!(DEFAULT_MESSAGE, "Back soon");
        assert_eq!(check_message(DEFAULT_MESSAGE), Ok(()));
        assert_eq!(check_message(""), Ok(()));
        // Characters, not bytes: each é is two.
        assert_eq!(check_message(&"é".repeat(200)), Ok(()));
        assert_eq!(
            check_message(&"é".repeat(201)),
            Err(CommandError::Usage(commands::MAINTENANCE))
        );
        for hidden in [
            "\u{1b}[2Jgone",
            "\u{1b}]0;owned\u{7}",
            "back \u{202E}soon",
            "zero\u{200B}width",
            "two\nlines",
        ] {
            assert_eq!(
                check_message(hidden),
                Err(CommandError::NotPlain),
                "{hidden:?}"
            );
        }
    }

    #[test]
    fn the_default_message_reads_well_after_the_launchers_words() {
        // The launcher leads with "in maintenance" itself, so the default says what's next.
        use fenji_launcher::api::ApiError;
        use fenji_launcher::app::words::api_error;
        let e = ApiError::Maintenance {
            message: DEFAULT_MESSAGE.into(),
            until: None,
        };
        assert_eq!(e.to_string(), "the server is in maintenance: Back soon");
        assert_eq!(
            api_error(&e, "Moonward", None, None),
            "Moonward is in maintenance: Back soon."
        );
    }

    #[tokio::test]
    async fn a_new_server_is_online_with_the_files_motd_and_nobody_in_the_world() {
        let (_d, store) = store().await;
        let s = Status::load(&store, NOW, "Hello").await.unwrap();
        assert_eq!(s.started_ms(), NOW);
        assert_eq!(s.maintenance(), None);
        assert_eq!(s.motd(), "Hello");
        assert!(s.names().is_empty());
    }

    #[tokio::test]
    async fn maintenance_is_stored_then_published() {
        let (_d, store) = store().await;
        let s = Status::load(&store, NOW, "").await.unwrap();
        let mut seen = s.watch_maintenance();
        let m = updating(Some(NOW + 60_000));
        s.set_maintenance(&store, Some(m.clone())).await.unwrap();
        assert!(seen.has_changed().unwrap());
        assert_eq!(*seen.borrow_and_update(), Some(m.clone()));
        let stored = store.state("maintenance").await.unwrap().unwrap();
        assert_eq!(serde_json::from_str::<Maintenance>(&stored).unwrap(), m);
        s.set_maintenance(&store, None).await.unwrap();
        assert_eq!(*seen.borrow_and_update(), None);
        assert_eq!(s.maintenance(), None);
        assert_eq!(store.state("maintenance").await.unwrap(), None);
    }

    #[tokio::test]
    async fn text_that_isnt_plain_or_is_too_long_changes_nothing() {
        let (_d, store) = store().await;
        let s = Status::load(&store, NOW, "").await.unwrap();
        for message in ["\u{1b}]0;owned\u{7}".to_string(), "x".repeat(201)] {
            let m = Maintenance {
                message,
                ..updating(None)
            };
            assert!(s.set_maintenance(&store, Some(m)).await.is_err());
            assert!(s.set_motd(&store, Some("a\u{202E}b".into())).await.is_err());
            assert!(s.set_motd(&store, Some("x".repeat(201))).await.is_err());
        }
        assert_eq!(s.maintenance(), None);
        assert_eq!(s.motd(), "");
        assert_eq!(store.state("maintenance").await.unwrap(), None);
        assert_eq!(store.state("motd").await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_maintenance_this_version_cant_read_keeps_the_server_in_maintenance() {
        // Another version's value, or a hand edit: the host meant the server shut, and can turn
        // maintenance off from the console.
        let (_d, store) = store().await;
        store.set_state("maintenance", "not json").await.unwrap();
        let s = Status::load(&store, NOW, "").await.unwrap();
        assert_eq!(
            s.maintenance(),
            Some(Maintenance {
                message: DEFAULT_MESSAGE.into(),
                since_ms: NOW,
                until_ms: None,
            })
        );
    }

    #[tokio::test]
    async fn the_motd_is_the_databases_when_it_has_one_else_the_files() {
        let (_d, store) = store().await;
        let s = Status::load(&store, NOW, "From the file").await.unwrap();
        let mut seen = s.watch_motd();
        s.set_motd(&store, Some("From the console".into()))
            .await
            .unwrap();
        assert_eq!(*seen.borrow_and_update(), "From the console");
        let restarted = Status::load(&store, NOW, "From the file").await.unwrap();
        assert_eq!(restarted.motd(), "From the console");
        s.set_motd(&store, Some(String::new())).await.unwrap();
        assert_eq!(*seen.borrow_and_update(), "", "an empty one is none");
        s.set_motd(&store, None).await.unwrap();
        assert_eq!(*seen.borrow_and_update(), "From the file");
        assert_eq!(store.state("motd").await.unwrap(), None);
    }

    #[tokio::test]
    async fn the_names_are_what_the_world_last_sent() {
        let (_d, store) = store().await;
        let s = Status::load(&store, NOW, "").await.unwrap();
        let names = s.names_sender();
        names.send_replace(Arc::from(vec!["Lin".to_string(), "Mai".to_string()]));
        assert_eq!(&*s.names(), ["Lin".to_string(), "Mai".to_string()]);
        names.send_replace(Arc::from(Vec::new()));
        assert!(s.names().is_empty());
    }
}
