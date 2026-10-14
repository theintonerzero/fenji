//! Entering the world (docs/protocol/packets/world.md; M4 world plan Amendment A): 772, which loads
//! the scene; the client's type 3 once it has; time syncs both ways; then the player's own actor
//! (type 7 and its property blob), its speed (type 23) and type 8. Native bodies go out with
//! `header::write_message` under their type; 772 is a script message, built by `script_body`,
//! which refuses `NEVER_SEND`. This module builds and parses; the server decides. Functions are
//! the pinned exe's; their REA Evidence IDs are in world.md's REA Evidence IDs table.
use crate::lobby::Appearance;
use crate::opcode::Cts;
use crate::packet::script_body;
use crate::pickle::{self, Table, Value};
use crate::{Error, Result};

/// `PROTOS2C_ACTOR_LOAD_SCENE` (`scene_manager.luac` `0.56`; world.md §2).
pub const LOAD_SCENE: i64 = 772;
/// The scripts' property-schema constant, which 772's handler compares with its fifth value
/// (world.md 2.2; a mismatch only logs).
pub const PROPS_VERSION: i64 = 4_706_187_382_722_034_291;
/// Type 3's body: `send_enter` (FUN_140221BB0) posts 21 bytes (world.md 3.1).
pub const CLIENT_ENTER_LEN: usize = 21;
/// A client time sync's body: `send_time_sync` (FUN_140221AC0) posts 13 bytes (world.md 7.2).
pub const TIME_SYNC_LEN: usize = 13;
/// `prop_move_speed`, the index type 23 takes for the speed (FUN_14020CE90; world.md 6.2).
pub const MOVE_SPEED: u16 = 1325;
/// The player class's properties (BUILTIN_PROPS, FUN_1401AC960; world.md 4.8). A blob entry with
/// a higher index raises in the client, and the entries after it are never set (4.5).
pub const PROPERTIES: u16 = 2439;
/// The health the player's own actor shows, as the most and the current (properties 304 and
/// 1458, with 300's ratio at 1.0). M4's world has no combat, so every player stands at full
/// health; the value is Fenji's choice. Any current health above 0 keeps the client from drawing
/// the player dead: with 1458 unset, run m4-e1 showed "HP 0" and a grey scene (world.md 5.6).
pub const FULL_HEALTH: i64 = 1000;
/// Type 7's header: the id, the actor type, the difficulty and the template (world.md 4.1).
const ENTER_HEADER: usize = 14;
/// The actor type of a player, and the difficulty the select screen's preview uses (world.md 5.4).
const PLAYER: u8 = 1;
const DIFFICULTY: u8 = 255;

/// The property indexes the player's own type 7 sets (world.md §5): BUILTIN_PROPS' indexes, which
/// the scripts' numeric constants use (4.8).
pub mod prop {
    pub const NAME: u16 = 1;
    pub const GENDER: u16 = 4;
    pub const VOCATION: u16 = 5;
    pub const SKELETON: u16 = 6;
    pub const LEVEL: u16 = 9;
    pub const POSITION: u16 = 16;
    pub const PORTRAIT: u16 = 105;
    pub const SHOW_EQUIP_HEAD: u16 = 109;
    pub const RESULT_SHOW_EQUIP_HEAD: u16 = 128;
    pub const VOICE: u16 = 177;
    pub const AVATAR_FEATURE: u16 = 214;
    pub const AVATAR_FEATURE_FACE: u16 = 215;
    pub const EQUIPS: u16 = 222;
    pub const HP_RATIO: u16 = 300;
    pub const HP_MAX: u16 = 304;
    pub const MOVE_SPEED: u16 = super::MOVE_SPEED;
    pub const HP: u16 = 1458;
}

/// The blob's tags: the jump table at 0x14017BD64 in FUN_14017BA60 (world.md §4's tag table).
mod tag {
    pub const NIL: u8 = 0;
    pub const FALSE: u8 = 1;
    pub const TRUE: u8 = 2;
    pub const I8: u8 = 3;
    pub const I16: u8 = 4;
    pub const I32: u8 = 5;
    pub const I64: u8 = 6;
    pub const F64: u8 = 7;
    pub const VEC: u8 = 8;
    /// Strings: a `u8`, `u16` or `u32` length, the bytes, then a byte the reader overwrites with 0.
    pub const STR: [u8; 3] = [9, 10, 11];
    /// Pickled values: a `u8`, `u16` or `u32` length, then a pickle packet of one value.
    pub const PICKLED: [u8; 3] = [12, 13, 14];
    /// Ends the blob in the client; Fenji never writes it, and `decode_blob` refuses it.
    pub const END: u8 = 15;
}

/// 772: load scene `scene` and stand at `pos` (x, y, z and a yaw), as the actor `pid` (world.md
/// §2, §11). The arguments are the pid; the scene table, with `scene_uid` (the instance) and
/// `scene_tid` (the scene) both `scene`, and `scene_lv` and `scene_type` 0; the position as four
/// floats; and [`PROPS_VERSION`].
pub fn load_scene(pid: i64, scene: i64, pos: [f64; 4]) -> Vec<u8> {
    let key = |k: &str| Value::Str(k.as_bytes().to_vec());
    let sinfo = Table {
        array: Vec::new(),
        hash: vec![
            (key("scene_uid"), Value::Int(scene)),
            (key("scene_tid"), Value::Int(scene)),
            (key("scene_lv"), Value::Int(0)),
            (key("scene_type"), Value::Int(0)),
        ],
    };
    let at = Table {
        array: pos.iter().map(|c| Value::Float(*c)).collect(),
        hash: Vec::new(),
    };
    script_body(
        LOAD_SCENE,
        &[
            Value::Int(pid),
            Value::Table(sinfo),
            Value::Table(at),
            Value::Int(PROPS_VERSION),
        ],
    )
    .expect("five values and two flat tables")
}

/// Type 3, `CTS_PROTOCOL_CLIENT_ENTER`: the client has loaded the scene. Only its length is
/// checked: the rest describes the player's machine (its Windows version and its parent process),
/// which Fenji neither reads nor keeps (world.md 3.2).
pub fn client_enter(body: &[u8]) -> Result<()> {
    if body.len() == CLIENT_ENTER_LEN {
        Ok(())
    } else {
        Err(Error::BadBody {
            opcode: Cts::ClientEnter.to_u8(),
            len: body.len(),
        })
    }
}

/// Type 2, `CTS_PROTOCOL_TIME_SYNC`: the low 32 bits of the client's millisecond clock, the `u32`
/// at offset 0 of 13 bytes. The two counters after it describe the player's machine and are never
/// read (world.md 7.2).
pub fn time_sync_request(body: &[u8]) -> Result<u32> {
    match body {
        [a, b, c, d, ..] if body.len() == TIME_SYNC_LEN => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => Err(Error::BadBody {
            opcode: Cts::TimeSync.to_u8(),
            len: body.len(),
        }),
    }
}

/// Type 2, `STC_PROTOCOL_TIME_SYNC` (FUN_140222E00's case 2; world.md 7.3–7.5): the client's `u32`
/// echoed, then the server's Unix time in milliseconds.
pub fn time_sync_reply(echo: u32, server_ms: u64) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[..4].copy_from_slice(&echo.to_le_bytes());
    out[4..].copy_from_slice(&server_ms.to_le_bytes());
    out
}

/// A property value in a blob (world.md §4's tag table).
#[derive(Debug, Clone, PartialEq)]
pub enum BlobValue {
    Nil,
    Bool(bool),
    /// Tags 3–6. Written in the tag its absolute value needs, as the client's pickle packer picks
    /// one (payloads.md P.3), so −128 takes tag 4; read from any of them.
    Int(i64),
    Num(f64),
    /// Tag 8: x, y, z and a yaw, as `f32` (world.md 4.12).
    Vec([f32; 4]),
    /// Tags 9–11.
    Str(Vec<u8>),
    /// Tags 12–14: a pickle packet, which must hold exactly one value (world.md 4.5, 4.7).
    Pickled(Vec<Value>),
}

/// The tag for a length in its three widths (`u8`, `u16`, `u32`), and the length's bytes.
fn sized(tags: [u8; 3], len: usize) -> Result<(u8, Vec<u8>)> {
    if let Ok(n) = u8::try_from(len) {
        Ok((tags[0], vec![n]))
    } else if let Ok(n) = u16::try_from(len) {
        Ok((tags[1], n.to_le_bytes().to_vec()))
    } else if let Ok(n) = u32::try_from(len) {
        Ok((tags[2], n.to_le_bytes().to_vec()))
    } else {
        Err(Error::Blob(format!("{len} bytes don't fit a u32 length")))
    }
}

/// Appends one entry: a `u16` holding the index in its low 12 bits and the tag in its top 4, then
/// the value (FUN_1401ACD10, FUN_14017BA60; world.md 4.4–4.7). Refuses what the client couldn't
/// set: index 0, which ends the blob, an index past [`PROPERTIES`], and a pickle that isn't one
/// value. Nothing is written for a refused entry.
pub fn put_entry(out: &mut Vec<u8>, index: u16, value: &BlobValue) -> Result<()> {
    if index == 0 || index > PROPERTIES {
        return Err(Error::Blob(format!(
            "property index {index} is outside 1 to {PROPERTIES}"
        )));
    }
    let (tag, data) = match value {
        BlobValue::Nil => (tag::NIL, Vec::new()),
        BlobValue::Bool(false) => (tag::FALSE, Vec::new()),
        BlobValue::Bool(true) => (tag::TRUE, Vec::new()),
        BlobValue::Int(n) => {
            // A value that fits a width has that many low bytes of its two's complement.
            let bytes = n.to_le_bytes();
            let (tag, width) = match n.unsigned_abs() {
                0..0x80 => (tag::I8, 1),
                0x80..0x8000 => (tag::I16, 2),
                0x8000..0x8000_0000 => (tag::I32, 4),
                _ => (tag::I64, 8),
            };
            (tag, bytes[..width].to_vec())
        }
        BlobValue::Num(f) => (tag::F64, f.to_le_bytes().to_vec()),
        BlobValue::Vec(v) => (tag::VEC, v.iter().flat_map(|c| c.to_le_bytes()).collect()),
        BlobValue::Str(s) => {
            let (tag, mut data) = sized(tag::STR, s.len())?;
            data.extend_from_slice(s);
            // The reader overwrites the byte after a string with 0 and moves past it (4.6).
            data.push(0);
            (tag, data)
        }
        BlobValue::Pickled(values) => {
            if values.len() != 1 {
                return Err(Error::Blob(format!(
                    "a pickled property holds one value, not {}",
                    values.len()
                )));
            }
            let packet = pickle::encode(values)?;
            let (tag, mut data) = sized(tag::PICKLED, packet.len())?;
            data.extend_from_slice(&packet);
            (tag, data)
        }
    };
    out.extend_from_slice(&((u16::from(tag) << 12) | index).to_le_bytes());
    out.extend_from_slice(&data);
    Ok(())
}

/// The entries of a property blob, as the client reads them (FUN_1401ACD10, FUN_14017BA60; world.md
/// 4.4–4.7), for tests and the protocol bot. Like the client, it stops at index 0, the end marker,
/// reads an integer in any width, and takes a last string without the byte after it. Unlike the
/// client, which stops quietly or raises, it refuses a tag-15 entry, an entry cut short, an index
/// past [`PROPERTIES`], and a pickle that isn't exactly one value.
pub fn decode_blob(blob: &[u8]) -> Result<Vec<(u16, BlobValue)>> {
    let mut r = BlobReader { blob, at: 0 };
    let mut out = Vec::new();
    while r.at < blob.len() {
        let head = u16::from_le_bytes(r.array()?);
        let (index, tag) = (head & 0x0FFF, (head >> 12) as u8);
        if index == 0 {
            break;
        }
        if index > PROPERTIES {
            return Err(Error::Blob(format!(
                "property index {index} is past the class's {PROPERTIES}"
            )));
        }
        let value = match tag {
            tag::NIL => BlobValue::Nil,
            tag::FALSE => BlobValue::Bool(false),
            tag::TRUE => BlobValue::Bool(true),
            tag::I8 => BlobValue::Int(i64::from(i8::from_le_bytes(r.array()?))),
            tag::I16 => BlobValue::Int(i64::from(i16::from_le_bytes(r.array()?))),
            tag::I32 => BlobValue::Int(i64::from(i32::from_le_bytes(r.array()?))),
            tag::I64 => BlobValue::Int(i64::from_le_bytes(r.array()?)),
            tag::F64 => BlobValue::Num(f64::from_le_bytes(r.array()?)),
            tag::VEC => {
                let mut v = [0f32; 4];
                for c in &mut v {
                    *c = f32::from_le_bytes(r.array()?);
                }
                BlobValue::Vec(v)
            }
            9..=11 => {
                let n = r.len(tag - 9)?;
                let s = r.take(n)?.to_vec();
                // The byte the reader overwrites with 0; a last string may lack it.
                r.at = (r.at + 1).min(blob.len());
                BlobValue::Str(s)
            }
            12..=14 => {
                let n = r.len(tag - 12)?;
                let values = pickle::decode(r.take(n)?)?;
                if values.len() != 1 {
                    return Err(Error::Blob(format!(
                        "property {index}'s pickle holds {} values, not one",
                        values.len()
                    )));
                }
                BlobValue::Pickled(values)
            }
            _ => {
                return Err(Error::Blob(format!(
                    "property {index} has tag {}, which ends the blob",
                    tag::END
                )));
            }
        };
        out.push((index, value));
    }
    Ok(out)
}

struct BlobReader<'a> {
    blob: &'a [u8],
    at: usize,
}

impl BlobReader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.blob.len());
        let Some(end) = end else {
            return Err(Error::Blob(format!("an entry cut short at {}", self.at)));
        };
        let got = &self.blob[self.at..end];
        self.at = end;
        Ok(got)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().expect("N bytes"))
    }

    /// A length of width 0 (`u8`), 1 (`u16`) or 2 (`u32`).
    fn len(&mut self, width: u8) -> Result<usize> {
        Ok(match width {
            0 => usize::from(u8::from_le_bytes(self.array()?)),
            1 => usize::from(u16::from_le_bytes(self.array()?)),
            _ => u32::from_le_bytes(self.array()?) as usize,
        })
    }
}

/// `CteateEquips[vocation][outfit]` from `vocation_initequip_templ`, as (slot, item id) pairs in
/// slot order (world.md §9): `EQUIP_SLOT_*` 2 chest, 3 wrist, 5 trousers, 6 boots and 11 weapon,
/// item ids `type << 24 | id`. Documented interoperability data by the owner's decision of
/// 2026-10-13 (M4 world spec §3.2, #180): a host's server can't read the client's tables. One
/// line per outfit, as world.md's table has them.
#[rustfmt::skip]
const OUTFITS: [[&[(i64, i64)]; 2]; 10] = [
    [&[(2, 100667850), (3, 100667855), (5, 100667870), (6, 100667860), (11, 83886281)],
     &[(2, 100664357), (3, 100664363), (5, 100664381), (6, 100664369), (11, 83886281)]],
    [&[(2, 100667851), (3, 100667856), (5, 100667871), (6, 100667861), (11, 83886282)],
     &[(2, 100664358), (3, 100664364), (5, 100664382), (6, 100664370), (11, 83886282)]],
    [&[(2, 100667852), (3, 100667857), (5, 100667872), (6, 100667862), (11, 83886283)],
     &[(2, 100664359), (3, 100664365), (5, 100664383), (6, 100664371), (11, 83886283)]],
    [&[(2, 100667853), (3, 100667858), (5, 100667873), (6, 100667863), (11, 83886284)],
     &[(2, 100664360), (3, 100664366), (5, 100664384), (6, 100664372), (11, 83886284)]],
    [&[(2, 100667853), (3, 100667858), (5, 100667873), (6, 100667863), (11, 83886285)],
     &[(2, 100664361), (3, 100664367), (5, 100664385), (6, 100664373), (11, 83886285)]],
    [&[(2, 100667854), (3, 100667859), (5, 100667874), (6, 100667864), (11, 83886286)],
     &[(2, 100664362), (3, 100664368), (5, 100664386), (6, 100664374), (11, 83886286)]],
    [&[(2, 100667850), (3, 100667855), (5, 100667870), (6, 100667860), (11, 83886287)],
     &[(2, 100665874), (3, 100665876), (5, 100665880), (6, 100665882), (11, 83886287)]],
    [&[(2, 100667853), (3, 100667858), (5, 100667873), (6, 100667863), (11, 83886288)],
     &[(2, 100665875), (3, 100665877), (5, 100665881), (6, 100665883), (11, 83886288)]],
    [&[(2, 100670186), (3, 100670187), (6, 100670190), (11, 83886289)],
     &[(2, 100670186), (3, 100670187), (6, 100670190), (11, 83886289)]],
    [&[(2, 100670202), (3, 100670207), (6, 100670222), (11, 83886287)],
     &[(2, 100665874), (3, 100665876), (5, 100665880), (6, 100665882), (11, 83886287)]],
];

/// The outfit a character of `vocation` was created in with the create screen's `outfit` button
/// (1 or 2), as (slot, item id) pairs in slot order; `None` outside vocations 0–9 and outfits 1–2.
/// The client never fills the outfit in itself (world.md 9.1), so the lobby and the world send it.
pub fn creation_outfit(vocation: i64, outfit: i64) -> Option<&'static [(i64, i64)]> {
    let v = usize::try_from(vocation).ok()?;
    let o = usize::try_from(outfit.checked_sub(1)?).ok()?;
    OUTFITS.get(v)?.get(o).copied()
}

/// An outfit as property 222 and the lobby's `prop_sys_equips` carry it: a table of slot → item
/// id, in slot order (world.md 9.3).
pub fn outfit_table(outfit: &[(i64, i64)]) -> Table {
    Table {
        array: Vec::new(),
        hash: outfit
            .iter()
            .map(|(slot, id)| (Value::Int(*slot), Value::Int(*id)))
            .collect(),
    }
}

/// The player's own actor, as its type 7 carries it (docs/protocol/packets/world.md §5).
pub struct SelfActor<'a> {
    pub pid: i64,
    pub name: &'a str,
    pub vocation: i64,
    pub skeleton: i64,
    pub level: i64,
    pub appearance: &'a Appearance,
    /// x, y, z and yaw, as in 772.
    pub pos: [f64; 4],
    pub speed: i64,
}

/// Type 7, `STC_PROTOCOL_SYNC_ENTER`, for the player's own actor (FUN_140222E00's case 7;
/// world.md §4, §5): the pid as a `u64`, the actor type 1, the difficulty 255 and the template
/// `vocation × 10000` as a `u32` (0 if that doesn't fit), then the blob, in index order. The head
/// is hidden, as the creation template hides it (properties 109 and 128 `false`; world.md 9.2);
/// the voice, the face and the outfit are left out when unset. The player stands at
/// [`FULL_HEALTH`] (300, 304 and 1458; world.md 5.6).
pub fn sync_enter_self(actor: &SelfActor) -> Vec<u8> {
    let a = actor.appearance;
    let mut entries = vec![
        (prop::NAME, BlobValue::Str(actor.name.as_bytes().to_vec())),
        (prop::GENDER, BlobValue::Int(i64::from(actor.skeleton != 1))),
        (prop::VOCATION, BlobValue::Int(actor.vocation)),
        (prop::SKELETON, BlobValue::Int(actor.skeleton)),
        (prop::LEVEL, BlobValue::Int(actor.level)),
        // Positions are f32 natively (world.md 2.8, 4.12).
        (prop::POSITION, BlobValue::Vec(actor.pos.map(|c| c as f32))),
        (prop::PORTRAIT, BlobValue::Int(a.portrait)),
        (prop::SHOW_EQUIP_HEAD, BlobValue::Bool(false)),
        (prop::RESULT_SHOW_EQUIP_HEAD, BlobValue::Bool(false)),
    ];
    if let Some(v) = a.voice {
        entries.push((prop::VOICE, BlobValue::Int(v)));
    }
    entries.push((prop::AVATAR_FEATURE, BlobValue::Int(a.base_face)));
    if let Some(f) = &a.face {
        entries.push((
            prop::AVATAR_FEATURE_FACE,
            BlobValue::Str(f.as_bytes().to_vec()),
        ));
    }
    if let Some(outfit) = creation_outfit(actor.vocation, a.equips_id) {
        let table = Value::Table(outfit_table(outfit));
        entries.push((prop::EQUIPS, BlobValue::Pickled(vec![table])));
    }
    entries.extend([
        (prop::HP_RATIO, BlobValue::Num(1.0)),
        (prop::HP_MAX, BlobValue::Int(FULL_HEALTH)),
        (prop::MOVE_SPEED, BlobValue::Int(actor.speed)),
        (prop::HP, BlobValue::Int(FULL_HEALTH)),
    ]);

    let mut out = Vec::with_capacity(ENTER_HEADER + 160);
    out.extend_from_slice(&actor.pid.to_le_bytes());
    out.push(PLAYER);
    out.push(DIFFICULTY);
    let template = actor
        .vocation
        .checked_mul(10_000)
        .and_then(|t| u32::try_from(t).ok())
        .unwrap_or(0);
    out.extend_from_slice(&template.to_le_bytes());
    for (index, value) in &entries {
        put_entry(&mut out, *index, value).expect("valid indexes, and bounded strings and tables");
    }
    out
}

/// Type 23, `STC_PROTOCOL_SYNC_PROP_FORCE`, setting the speed (FUN_14020CE90; world.md 6.2): the
/// pid as a `u64`, [`MOVE_SPEED`] as a `u16` and the rate as an `i16`, where 100 is 500 units a
/// second (6.4).
pub fn sync_prop_force_speed(pid: i64, rate: i16) -> [u8; 12] {
    let mut out = [0u8; 12];
    out[..8].copy_from_slice(&pid.to_le_bytes());
    out[8..10].copy_from_slice(&MOVE_SPEED.to_le_bytes());
    out[10..].copy_from_slice(&rate.to_le_bytes());
    out
}

/// Type 8, `STC_PROTOCOL_SYNC_ENTER_FINISH` (FUN_140222E00's case 8; world.md 6.1): the pid as a
/// `u64`. For the local actor it turns the client's type-0 sends back on (1.2).
pub fn sync_enter_finish(pid: i64) -> [u8; 8] {
    pid.to_le_bytes()
}
