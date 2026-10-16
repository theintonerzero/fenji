//! The lobby's script messages (docs/protocol/packets/lobby.md): the character list, creation and
//! deletion and the error message, as the client's own scripts send and handle them. Ids are the
//! literals in `data/core/modules/net/net.luac`, which the client's protocol table
//! (`data/caches/protocol_ver.new_cache`) names. Refusals carry `ID`s from the client's own
//! `errors` sheet. This module builds and parses; the server decides.
use std::ops::RangeInclusive;

use crate::Result;
use crate::packet::script_body;
use crate::pickle::{Table, Value};

/// `PROTOC2S_ROLE_LIST ()`: `QueryActors`, sent after 370 and 496 (net.luac `0.48`).
pub const QUERY_ACTORS: i64 = 292;
/// `PROTOC2S_ROLE_SELECT (pid)`: entering the world, which is M4's (net.luac `0.49`).
pub const SELECT_ACTOR: i64 = 10;
/// `PROTOC2S_ROLE_CREATE (table)` (net.luac `0.50`).
pub const CREATE_ACTOR: i64 = 304;
/// `PROTOC2S_ROLE_DELETE (pid)` (net.luac `0.51`).
pub const DELETE_ACTOR: i64 = 166;
/// `PROTOC2G_ROLE_SLOT (pid, slot[, other pid])`: the select screen renumbers or swaps (net.luac
/// `0.54`).
pub const CHANGE_SLOT: i64 = 321;
/// `PROTOC2G_HOTPATCH_INIT (revision)` (hotpatch_system.luac `0.4`); never answered.
pub const HOTPATCH_INIT: i64 = 370;
/// `PROTOC2G_GM_SCRIPTS ()`, sent after 370 when `is_shipping_client()` (hotpatch_system.luac
/// `0.4`); never answered, since its reply, 216, replaces scripts ([`NEVER_SEND`]).
pub const GM_SCRIPTS: i64 = 496;
/// `PROTOC2G_SERVER_LIST ()`, sent by the `SET_NET_CONNECTED` handler (net.luac `0.29`). Its
/// reply, 495, only sets the server-name label.
pub const SERVER_LIST: i64 = 402;
/// `PROTOC2S_CLIENT_EXIT ()`: the client is quitting. It quits 5 s later whatever the server does
/// (UIMain.luac `RequestQuitGame`), so the server only closes the connection.
pub const CLIENT_EXIT: i64 = 392;

/// `PROTOG2C_ROLE_LIST (last_pid, list, extra)` (net.luac `0.32`).
pub const ROLE_LIST: i64 = 228;
/// `PROTOG2C_ROLE_CREATE (actor)` (net.luac `0.33`).
pub const ROLE_CREATE: i64 = 628;
/// `PROTOG2C_ROLE_DELETE (pid, ms, level)` (net.luac `0.34`).
pub const ROLE_DELETE: i64 = 84;
/// `PROTOS2C_ERROR_CODE (code)` (err.luac `0.8`): the client prints it and fires
/// `EVENT_SERVER_ERROR`.
pub const ERROR_CODE: i64 = 270;
pub use crate::packet::NEVER_SEND;
/// The select screen shows `9 + role_num` places (locale/common ActorSelect.luac `0.3`).
pub const ROLE_SLOTS: u32 = 9;

/// Rows of the client's `errors` sheet, by `literal` (lobby.md §8).
pub mod code {
    pub const ROLE_NAME_EXIST: i64 = 5_391_046_069_930_669_593;
    pub const ROLE_NAME_LENERR: i64 = -2_076_552_961_395_212_067;
    pub const ROLE_NAME_ILLEGAL: i64 = -2_195_789_287_712_819_046;
    /// Its English text lists the Western server's permitted name characters.
    pub const ROLE_NAME_ALLCHINESE: i64 = -2_789_480_703_151_742_396;
    pub const ROLE_MAX_NUMBER: i64 = 8_663_116_806_817_837_223;
    pub const ROLE_NOT_FOUND: i64 = -2_776_944_664_988_636_544;
    pub const ROLE_VOC_SKELETON_NOT_MATCH: i64 = -7_591_047_980_372_978_664;
    /// "The server isn't open yet": M3's answer to entering the world.
    pub const SERVER_NOT_RUNNING: i64 = 7_248_304_305_738_093_764;
    /// "Server not responding": for the server's own failures.
    pub const SERVER_NOT_RESP: i64 = -5_759_203_401_640_685_215;
}

/// The model's show flags that hide their piece when missing (`x or false`, pregame_actor.luac
/// `0.0`), with the value each is sent with. The head is hidden, as the creation template hides it
/// (`CreateShowEquipHead` 0; world.md 9.2), and the world's type 7 hides it too; every other piece
/// is shown.
pub const SHOW_FLAGS: [(&str, bool); 11] = [
    ("prop_sys_show_equip_head", false),
    ("prop_sys_show_equip_artifact1", true),
    ("prop_sys_show_equip_artifact2", true),
    ("prop_sys_show_equip_chest", true),
    ("prop_sys_show_equip_wrist", true),
    ("prop_sys_show_equip_boot", true),
    ("prop_sys_show_equip_weapon", true),
    ("prop_sys_show_equip_pendantmoon", true),
    ("prop_sys_show_equip_pendantstar1", true),
    ("prop_sys_show_equip_pendantstar2", true),
    ("prop_sys_show_exterior", true),
];

/// A name's length in bytes of UTF-8. The client's box holds 24 bytes (FeatureSetting.xml
/// `maxBytes`); M3 asks for at least 2.
pub const NAME_BYTES: RangeInclusive<usize> = 2..=24;
/// The face string's limit. The client's own run to about 1–2 KiB (lobby.md §5).
pub const FACE_MAX: usize = 4096;
/// The portrait a character without one gets: the select screen's own default, `25 << 16`
/// (ActorFrame.luac `0.1.7`), as `portrait_id << 16 | frame`.
pub const DEFAULT_PORTRAIT: i64 = 25 << 16;

/// Why a create request is refused, and the client's code for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    NameLength,
    NameChars,
    VocationSkeleton,
    Malformed,
}

impl Refusal {
    pub fn code(self) -> i64 {
        match self {
            Self::NameLength => code::ROLE_NAME_LENERR,
            Self::NameChars => code::ROLE_NAME_ALLCHINESE,
            Self::VocationSkeleton => code::ROLE_VOC_SKELETON_NOT_MATCH,
            Self::Malformed => code::ROLE_NAME_ILLEGAL,
        }
    }
}

/// The appearance fields the server keeps, typed and bounded (M3 spec D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appearance {
    pub voice: Option<i64>,
    /// The base face, 1 or 2 (clamped as the client's `check_base_face` does; 1 when absent).
    pub base_face: i64,
    /// The face editor's string (`v:5;…`).
    pub face: Option<String>,
    /// The starting outfit, 1 or 2 (the create screen's two buttons; clamped, 1 when absent).
    pub equips_id: i64,
    /// `portrait_id << 16 | frame`.
    pub portrait: i64,
}

/// Message 304 as the server accepts it: a name that keeps to the Western rule, a vocation with
/// one of its skeletons, and the appearance fields typed and bounded. Fields the server doesn't
/// keep are ignored (lobby.md §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateRequest {
    pub name: String,
    pub vocation: i64,
    pub skeleton: i64,
    pub appearance: Appearance,
}

fn key(k: &Value) -> Option<&str> {
    match k {
        Value::Str(b) => std::str::from_utf8(b).ok(),
        _ => None,
    }
}

/// An integer, or a float with no fraction (the client's numbers can arrive either way).
fn int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Float(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(*f as i64),
        _ => None,
    }
}

/// The skeletons the Western create screen offers per vocation (`vocation_skel_templ`;
/// lobby.md §5). Vocation 8 is hidden in the Western build.
pub fn skeleton_allowed(vocation: i64, skeleton: i64) -> bool {
    matches!((vocation, skeleton), (0..=5, 1 | 2) | (6, 1) | (7 | 9, 3))
}

/// The letters with diacritics the Western server allowed (the English text of
/// `ERR_ROLE_NAME_ALLCHINESE`), in both cases: umlauts on a, e, i, o, u and y, grave accents on
/// a, e, i, o and u, acute accents on a, e, i, o, u and y, and the cedilla on c.
const DIACRITICS: &str = "äëïöüÿÄËÏÖÜŸàèìòùÀÈÌÒÙáéíóúýÁÉÍÓÚÝçÇ";

/// The Western server's characters (the English text of `ERR_ROLE_NAME_ALLCHINESE`): the letters
/// A–Z in either case, the letters in [`DIACRITICS`], full stops (not first or last), hyphens and
/// underscores. Never `$`: a name is a `locale_str`, whose setter on every client that sees the
/// player unpickles the rest of a value that starts with `$locale$` (world.md 4.10; #281).
fn name_chars_ok(name: &str) -> bool {
    let ok =
        |c: char| c.is_ascii_alphabetic() || DIACRITICS.contains(c) || matches!(c, '.' | '-' | '_');
    name.chars().all(ok) && !name.starts_with('.') && !name.ends_with('.')
}

impl CreateRequest {
    /// `args` are message 304's arguments: one table of scalars (lobby.md §5).
    pub fn parse(args: &[Value]) -> std::result::Result<Self, Refusal> {
        let Some(Value::Table(t)) = args.first() else {
            return Err(Refusal::Malformed);
        };
        let (mut name, mut vocation, mut skeleton) = (None, None, None);
        let mut a = Appearance {
            voice: None,
            base_face: 1,
            face: None,
            equips_id: 1,
            portrait: DEFAULT_PORTRAIT,
        };
        for (k, v) in &t.hash {
            match key(k) {
                Some("prop_base_name") => match v {
                    Value::Str(b) if !NAME_BYTES.contains(&b.len()) => {
                        return Err(Refusal::NameLength);
                    }
                    Value::Str(b) => {
                        name = Some(String::from_utf8(b.clone()).map_err(|_| Refusal::NameChars)?)
                    }
                    _ => return Err(Refusal::Malformed),
                },
                Some("prop_base_vocation") => vocation = Some(int(v).ok_or(Refusal::Malformed)?),
                Some("prop_base_skeleton") => skeleton = Some(int(v).ok_or(Refusal::Malformed)?),
                Some("prop_sys_player_voice") => {
                    a.voice = Some(
                        int(v)
                            .filter(|x| (0..=64).contains(x))
                            .ok_or(Refusal::Malformed)?,
                    )
                }
                // As the client's check_base_face (face_verification.luac 0.36): clamped to 1–2.
                // Absent, it stays 1; present, it must be a number.
                Some("prop_sys_avatar_feature") => {
                    a.base_face = int(v).ok_or(Refusal::Malformed)?.clamp(1, 2)
                }
                Some("prop_sys_avatar_feature_face") => match v {
                    Value::Str(b) if b.is_empty() => {}
                    Value::Str(b) if b.len() <= FACE_MAX && b.iter().all(u8::is_ascii_graphic) => {
                        a.face = Some(String::from_utf8(b.clone()).map_err(|_| Refusal::Malformed)?)
                    }
                    _ => return Err(Refusal::Malformed),
                },
                Some("prop_base_equips_id") => {
                    a.equips_id = int(v).ok_or(Refusal::Malformed)?.clamp(1, 2)
                }
                Some("prop_sys_portrait") => {
                    a.portrait = int(v)
                        .filter(|x| (0..=i64::from(i32::MAX)).contains(x))
                        .ok_or(Refusal::Malformed)?
                }
                Some("prop_sys_player_tattoo_pos" | "prop_sys_player_birthday") => {
                    int(v).ok_or(Refusal::Malformed)?;
                }
                _ => {}
            }
        }
        let name = name.ok_or(Refusal::Malformed)?;
        if !name_chars_ok(&name) {
            return Err(Refusal::NameChars);
        }
        let vocation = vocation.ok_or(Refusal::Malformed)?;
        let skeleton = skeleton.ok_or(Refusal::Malformed)?;
        if !skeleton_allowed(vocation, skeleton) {
            return Err(Refusal::VocationSkeleton);
        }
        Ok(Self {
            name,
            vocation,
            skeleton,
            appearance: a,
        })
    }
}

fn s(x: &str) -> Value {
    Value::Str(x.as_bytes().to_vec())
}

impl Appearance {
    /// As the server stores it: a flat table, which `pickle::encode` turns into bytes.
    pub fn to_table(&self) -> Table {
        let mut hash = vec![
            (s("base_face"), Value::Int(self.base_face)),
            (s("equips_id"), Value::Int(self.equips_id)),
            (s("portrait"), Value::Int(self.portrait)),
        ];
        if let Some(v) = self.voice {
            hash.push((s("voice"), Value::Int(v)));
        }
        if let Some(f) = &self.face {
            hash.push((s("face"), s(f)));
        }
        Table {
            array: Vec::new(),
            hash,
        }
    }

    /// The stored table back. Anything missing takes the create screen's default.
    pub fn from_table(t: &Table) -> Self {
        let get = |k: &str| {
            t.hash
                .iter()
                .find(|(kk, _)| key(kk) == Some(k))
                .map(|(_, v)| v)
        };
        Self {
            voice: get("voice").and_then(int),
            base_face: get("base_face").and_then(int).unwrap_or(1),
            face: get("face").and_then(|v| match v {
                Value::Str(b) => String::from_utf8(b.clone()).ok(),
                _ => None,
            }),
            equips_id: get("equips_id").and_then(int).unwrap_or(1),
            portrait: get("portrait").and_then(int).unwrap_or(DEFAULT_PORTRAIT),
        }
    }
}

/// One character as the select screen reads it (lobby.md §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub pid: i64,
    pub name: String,
    pub level: i64,
    pub vocation: i64,
    pub skeleton: i64,
    pub appearance: Appearance,
}

impl Actor {
    /// The outfit the character was created in, as the world shows it: the shipped creation outfit
    /// of its vocation and stored `equips_id` (world.md §9), since the client never fills it in
    /// (world.md 9.1). Outside that table, `{}`, which M3 sent.
    fn outfit(&self) -> Table {
        crate::world::creation_outfit(self.vocation, self.appearance.equips_id)
            .map(crate::world::outfit_table)
            .unwrap_or_default()
    }

    /// The actor's table in place `slot` (1-based). It never carries `enable`, which would break
    /// the select screen's slot builder, or `block_cd`, which shows a character as blocked while
    /// the lobby's server time is 0 (lobby.md §4).
    pub fn to_table(&self, slot: i64) -> Table {
        let gender = if self.skeleton == 1 { 0 } else { 1 };
        let mut hash = vec![
            (s("pid"), Value::Int(self.pid)),
            (s("slot"), Value::Int(slot)),
            (s("prop_base_name"), s(&self.name)),
            (s("prop_base_lv"), Value::Int(self.level)),
            (s("prop_base_vocation"), Value::Int(self.vocation)),
            (s("prop_base_gender"), Value::Int(gender)),
            (s("prop_base_skeleton"), Value::Int(self.skeleton)),
            (s("prop_sys_portrait"), Value::Int(self.appearance.portrait)),
            (
                s("prop_sys_avatar_feature"),
                Value::Int(self.appearance.base_face),
            ),
            (s("prop_sys_equips"), Value::Table(self.outfit())),
        ];
        if let Some(f) = &self.appearance.face {
            hash.push((s("prop_sys_avatar_feature_face"), s(f)));
        }
        hash.extend(
            SHOW_FLAGS
                .iter()
                .map(|(f, shown)| (s(f), Value::Bool(*shown))),
        );
        Table {
            array: Vec::new(),
            hash,
        }
    }
}

/// 228's third argument (lobby.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccountExtras {
    /// Places beyond the select screen's nine.
    pub role_num: i64,
}

/// 228: the pid to pre-select (0: slot 1), the actors in slots that follow the list's order, and
/// the account's extras.
pub fn role_list(last_pid: i64, actors: &[Actor], extras: &AccountExtras) -> Result<Vec<u8>> {
    let list = Table {
        array: actors
            .iter()
            .zip(1..)
            .map(|(a, slot)| Value::Table(a.to_table(slot)))
            .collect(),
        hash: Vec::new(),
    };
    let extra = Table {
        array: Vec::new(),
        hash: vec![
            (s("rename_num"), Value::Int(0)),
            (s("reborn_num"), Value::Int(0)),
            (s("change_gender_num"), Value::Int(0)),
            (s("role_num"), Value::Int(extras.role_num)),
        ],
    };
    script_body(
        ROLE_LIST,
        &[
            Value::Int(last_pid),
            Value::Table(list),
            Value::Table(extra),
        ],
    )
}

/// 628: the new actor, in the place after the account's other characters (lobby.md §6).
pub fn create_result(actor: &Actor, slot: i64) -> Result<Vec<u8>> {
    script_body(ROLE_CREATE, &[Value::Table(actor.to_table(slot))])
}

/// 84: the pid, no pending time, and a level threshold no character reaches, so the client shows
/// no "pending deletion" dialog and asks for the list again (lobby.md §7; M3 spec D9).
pub fn delete_result(pid: i64) -> Vec<u8> {
    script_body(
        ROLE_DELETE,
        &[Value::Int(pid), Value::Int(0), Value::Int(2_147_483_647)],
    )
    .expect("four integers")
}

/// 270 with one of [`code`]'s values.
pub fn error_code(code: i64) -> Vec<u8> {
    script_body(ERROR_CODE, &[Value::Int(code)]).expect("two integers")
}
