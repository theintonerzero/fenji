//! The lobby (M3 spec D8–D11; docs/protocol/packets/lobby.md). It lists, creates and deletes
//! against the store, and ignores the rest: 402, 370, 496 and 321 need no answer. The gate takes
//! 392 (`CLIENT_EXIT`), which closes the connection, and 10 (`SELECT_ACTOR`), which enters the
//! world, before they get here. Every 292 gets a 228, every 304 a 628 or a 270, and every 166 an
//! 84 or a 270, because the client's screens wait for them (lobby.md §6–§7). Replies are built by
//! `fenji_proto::lobby`, whose `script_body` refuses `NEVER_SEND`.
use fenji_proto::lobby::{self, AccountExtras, Actor, Appearance, CreateRequest, code};
use fenji_proto::pickle::{self, Table, Value};
use fenji_store::{Account, Character, NewCharacter, Store};

pub struct LobbyLimits {
    pub max_characters: u32,
}

impl LobbyLimits {
    fn extras(&self) -> AccountExtras {
        AccountExtras {
            role_num: i64::from(self.max_characters.saturating_sub(lobby::ROLE_SLOTS)),
        }
    }
}

/// A character's appearance as the store keeps it: the pickle of one table (M3 spec D8). `None`
/// when it doesn't decode.
pub(crate) fn stored_appearance(stored: &[u8]) -> Option<Appearance> {
    match pickle::decode(stored).ok()?.pop() {
        Some(Value::Table(t)) => Some(Appearance::from_table(&t)),
        _ => None,
    }
}

fn actor(c: &Character) -> Actor {
    let appearance = stored_appearance(&c.appearance)
        .unwrap_or_else(|| Appearance::from_table(&Table::default()));
    Actor {
        pid: c.pid,
        name: c.name.clone(),
        level: c.level,
        vocation: c.vocation,
        skeleton: c.skeleton,
        appearance,
    }
}

/// A pid as the client sends it back: an integer, or a float with no fraction.
pub(crate) fn pid(v: Option<&Value>) -> Option<i64> {
    match v? {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
        _ => None,
    }
}

/// The replies to script message `id` from `account`, in order.
pub async fn handle(
    store: &Store,
    limits: &LobbyLimits,
    account: &Account,
    now: i64,
    id: i64,
    args: &[Value],
) -> Vec<Vec<u8>> {
    let reply = match id {
        lobby::QUERY_ACTORS => list(store, limits, account).await,
        lobby::CREATE_ACTOR => create(store, limits, account, now, args).await,
        lobby::DELETE_ACTOR => delete(store, account, args).await,
        other => {
            tracing::debug!(id = other, "script message ignored");
            Ok(Vec::new())
        }
    };
    reply.unwrap_or_else(|e| {
        // The screens wait for an answer: give one, and log the cause.
        tracing::error!(error = %e, id, "lobby store error");
        vec![lobby::error_code(code::SERVER_NOT_RESP)]
    })
}

async fn list(
    store: &Store,
    limits: &LobbyLimits,
    account: &Account,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let actors: Vec<Actor> = store
        .list_characters(account.id)
        .await?
        .iter()
        .map(actor)
        .collect();
    Ok(vec![lobby::role_list(0, &actors, &limits.extras())?])
}

async fn create(
    store: &Store,
    limits: &LobbyLimits,
    account: &Account,
    now: i64,
    args: &[Value],
) -> anyhow::Result<Vec<Vec<u8>>> {
    let req = match CreateRequest::parse(args) {
        Ok(r) => r,
        Err(refusal) => {
            tracing::debug!(account = %account.username, ?refusal, "create refused");
            return Ok(vec![lobby::error_code(refusal.code())]);
        }
    };
    tracing::debug!(
        account = %account.username,
        name = %req.name,
        vocation = req.vocation,
        skeleton = req.skeleton,
        face_len = req.appearance.face.as_ref().map_or(0, String::len),
        "create"
    );
    let new = NewCharacter {
        name: req.name,
        vocation: req.vocation,
        skeleton: req.skeleton,
        appearance: pickle::encode(&[Value::Table(req.appearance.to_table())])?,
    };
    match store
        .create_character(account.id, &new, limits.max_characters, now)
        .await
    {
        Ok(c) => {
            let slot = i64::try_from(store.list_characters(account.id).await?.len())?;
            Ok(vec![lobby::create_result(&actor(&c), slot)?])
        }
        Err(fenji_store::Error::NameTaken) => Ok(vec![lobby::error_code(code::ROLE_NAME_EXIST)]),
        Err(fenji_store::Error::TooMany) => Ok(vec![lobby::error_code(code::ROLE_MAX_NUMBER)]),
        Err(e) => Err(e.into()),
    }
}

async fn delete(store: &Store, account: &Account, args: &[Value]) -> anyhow::Result<Vec<Vec<u8>>> {
    let Some(pid) = pid(args.first()) else {
        return Ok(vec![lobby::error_code(code::ROLE_NOT_FOUND)]);
    };
    Ok(vec![if store.delete_character(account.id, pid).await? {
        lobby::delete_result(pid)
    } else {
        lobby::error_code(code::ROLE_NOT_FOUND)
    }])
}

#[cfg(test)]
mod tests {
    use fenji_proto::lobby::{SHOW_FLAGS, code};
    use fenji_proto::pickle::{Table, Value};
    use fenji_store::{Account, Store};

    use super::*;

    const NOW: i64 = 1_800_000_000_000;

    async fn setup() -> (tempfile::TempDir, Store, Account) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("f.db")).await.unwrap();
        let code = fenji_auth::invite::InviteCode::generate().unwrap();
        store
            .create_invite(&code.hash(), None, NOW, None)
            .await
            .unwrap();
        store
            .redeem_invite(&code.hash(), "tester", "$argon2id$x", NOW)
            .await
            .unwrap();
        let account = store.account_by_username("tester").await.unwrap().unwrap();
        (dir, store, account)
    }

    fn s(x: &str) -> Value {
        Value::Str(x.as_bytes().to_vec())
    }

    fn request(name: &str) -> Vec<Value> {
        vec![Value::Table(Table {
            array: Vec::new(),
            hash: vec![
                (s("prop_base_name"), s(name)),
                (s("prop_base_vocation"), Value::Int(0)),
                (s("prop_base_skeleton"), Value::Int(2)),
                (s("prop_sys_player_voice"), Value::Int(4)),
                (s("prop_sys_avatar_feature"), Value::Int(2)),
                (s("prop_sys_avatar_feature_face"), s("v:5;2:1")),
                (s("prop_base_equips_id"), Value::Int(2)),
                (s("prop_sys_portrait"), Value::Int(24 << 16)),
            ],
        })]
    }

    const NINE: LobbyLimits = LobbyLimits { max_characters: 9 };

    /// A short string as the pickle writes it: tag 08, a length byte, the bytes.
    fn st(x: &str) -> Vec<u8> {
        let mut v = vec![0x08, u8::try_from(x.len()).unwrap()];
        v.extend_from_slice(x.as_bytes());
        v
    }

    /// A string key and a one-byte integer value.
    fn kv8(k: &str, n: i8) -> Vec<u8> {
        let mut v = st(k);
        v.extend([0x03, n.to_le_bytes()[0]]);
        v
    }

    #[tokio::test]
    async fn an_empty_account_lists_byte_for_byte() {
        let (_d, store, account) = setup().await;
        let out = handle(&store, &NINE, &account, NOW, lobby::QUERY_ACTORS, &[]).await;
        // 4 values: 228 (i16), last_pid 0, an empty list, the extras table.
        let mut want = vec![0x04, 0x04, 0xE4, 0x00, 0x03, 0x00];
        want.extend([0x0B, 0, 0, 0, 0, 0x00]);
        want.extend([0x0B, 0, 0, 0, 0]);
        for k in ["rename_num", "reborn_num", "change_gender_num", "role_num"] {
            want.extend(kv8(k, 0));
        }
        want.push(0x00);
        assert_eq!(out, vec![want]);
    }

    #[tokio::test]
    async fn a_create_answers_byte_for_byte_and_a_delete_too() {
        let (_d, store, account) = setup().await;
        let out = handle(
            &store,
            &NINE,
            &account,
            NOW,
            lobby::CREATE_ACTOR,
            &request("Lin"),
        )
        .await;
        // 2 values: 628 (i16) and the actor's table, in `Actor::to_table`'s order.
        let mut want = vec![0x02, 0x04, 0x74, 0x02, 0x0B, 0, 0, 0, 0];
        want.extend(kv8("pid", 1));
        want.extend(kv8("slot", 1));
        want.extend(st("prop_base_name"));
        want.extend(st("Lin"));
        want.extend(kv8("prop_base_lv", 37));
        want.extend(kv8("prop_base_vocation", 0));
        want.extend(kv8("prop_base_gender", 1));
        want.extend(kv8("prop_base_skeleton", 2));
        want.extend(st("prop_sys_portrait"));
        want.extend([0x05, 0x00, 0x00, 0x18, 0x00]);
        want.extend(kv8("prop_sys_avatar_feature", 2));
        // Outfit 2 of vocation 0 (world.md §9): each slot (i8) and its item id (i32).
        want.extend(st("prop_sys_equips"));
        want.extend([0x0B, 0, 0, 0, 0]);
        for (slot, id) in [
            (2, 100_664_357),
            (3, 100_664_363),
            (5, 100_664_381),
            (6, 100_664_369),
            (11, 83_886_281_i32),
        ] {
            want.extend([0x03, slot, 0x05]);
            want.extend(id.to_le_bytes());
        }
        want.push(0x00);
        want.extend(st("prop_sys_avatar_feature_face"));
        want.extend(st("v:5;2:1"));
        // The head hidden, as the creation template hides it (world.md 9.2); the rest shown.
        assert_eq!(SHOW_FLAGS[0].0, "prop_sys_show_equip_head");
        want.extend(st("prop_sys_show_equip_head"));
        want.push(0x02);
        for (f, _) in &SHOW_FLAGS[1..] {
            want.extend(st(f));
            want.push(0x01);
        }
        want.push(0x00);
        assert_eq!(out, vec![want]);

        let out = handle(
            &store,
            &NINE,
            &account,
            NOW,
            lobby::DELETE_ACTOR,
            &[Value::Int(1)],
        )
        .await;
        // 84, the pid, no pending time, a level threshold no one reaches (i32::MAX).
        let want = vec![
            0x04, 0x03, 0x54, 0x03, 0x01, 0x03, 0x00, 0x05, 0xFF, 0xFF, 0xFF, 0x7F,
        ];
        assert_eq!(out, vec![want]);
    }

    #[tokio::test]
    async fn an_unknown_pid_is_refused_byte_for_byte() {
        let (_d, store, account) = setup().await;
        for args in [vec![Value::Int(42)], vec![], vec![s("1")]] {
            let out = handle(&store, &NINE, &account, NOW, lobby::DELETE_ACTOR, &args).await;
            let mut want = vec![0x02, 0x04, 0x0E, 0x01, 0x06];
            want.extend(code::ROLE_NOT_FOUND.to_le_bytes());
            assert_eq!(out, vec![want], "{args:?}");
        }
    }

    #[tokio::test]
    async fn a_pid_may_come_back_as_a_float() {
        let (_d, store, account) = setup().await;
        handle(
            &store,
            &NINE,
            &account,
            NOW,
            lobby::CREATE_ACTOR,
            &request("Lin"),
        )
        .await;
        let out = handle(
            &store,
            &NINE,
            &account,
            NOW,
            lobby::DELETE_ACTOR,
            &[Value::Float(1.0)],
        )
        .await;
        assert_eq!(out, vec![lobby::delete_result(1)]);
    }

    #[tokio::test]
    async fn another_accounts_character_is_not_found() {
        let (_d, store, account) = setup().await;
        handle(
            &store,
            &NINE,
            &account,
            NOW,
            lobby::CREATE_ACTOR,
            &request("Lin"),
        )
        .await;
        let stranger = Account {
            id: account.id + 1,
            ..account.clone()
        };
        let out = handle(
            &store,
            &NINE,
            &stranger,
            NOW,
            lobby::DELETE_ACTOR,
            &[Value::Int(1)],
        )
        .await;
        assert_eq!(out, vec![lobby::error_code(code::ROLE_NOT_FOUND)]);
        assert_eq!(store.list_characters(account.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_extra_places_beyond_nine_are_role_num() {
        let (_d, store, account) = setup().await;
        let limits = LobbyLimits { max_characters: 12 };
        let out = handle(&store, &limits, &account, NOW, lobby::QUERY_ACTORS, &[]).await;
        let v = pickle::decode(&out[0]).unwrap();
        let Value::Table(extra) = &v[3] else { panic!() };
        let role_num = extra.hash.iter().find(|(k, _)| *k == s("role_num"));
        assert_eq!(role_num.map(|(_, v)| v), Some(&Value::Int(3)));
    }

    #[tokio::test]
    async fn ignored_ids_get_no_answer() {
        // 392 and 10 are the gate's: it closes on the one and enters the world on the other.
        let (_d, store, account) = setup().await;
        for id in [
            lobby::SERVER_LIST,
            lobby::HOTPATCH_INIT,
            lobby::GM_SCRIPTS,
            lobby::CHANGE_SLOT,
            lobby::CLIENT_EXIT,
            lobby::SELECT_ACTOR,
            12_345,
        ] {
            assert!(
                handle(&store, &NINE, &account, NOW, id, &[Value::Int(1)])
                    .await
                    .is_empty(),
                "{id}"
            );
        }
    }

    #[test]
    fn a_stored_appearance_reads_back_or_is_none() {
        let a = Appearance {
            voice: Some(3),
            base_face: 1,
            face: Some("v:5;".into()),
            equips_id: 2,
            portrait: 169 << 16,
        };
        let stored = pickle::encode(&[Value::Table(a.to_table())]).unwrap();
        assert_eq!(stored_appearance(&stored), Some(a));
        assert_eq!(stored_appearance(&[1, 2, 3]), None);
        assert_eq!(
            stored_appearance(&pickle::encode(&[Value::Int(1)]).unwrap()),
            None
        );
    }

    #[tokio::test]
    async fn a_store_failure_still_answers() {
        let (_d, store, account) = setup().await;
        store.close().await;
        let out = handle(&store, &NINE, &account, NOW, lobby::QUERY_ACTORS, &[]).await;
        assert_eq!(out, vec![lobby::error_code(code::SERVER_NOT_RESP)]);
    }
}
