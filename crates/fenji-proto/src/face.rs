//! The face string's shape (lobby.md 5.9–5.11; #391). The client's shared validator,
//! `check_feature_face` (`data/core/share/face_verification.luac`, S15 `0.45`, its parser `0.35`),
//! rebuilds a face from its sections and falls back to a default face when one fails. But it runs
//! only on the select screen and in the shop: in the world, a client hands another player's face
//! straight from property 215 to the native `set_face` (lobby.md 5.11, S30). So Fenji passes on
//! only faces in a shape that holds every face the client writes: the validator's sections, counts
//! and masks, which the run m3-l3b default face had exactly.
//!
//! The validator also clamps every value to the game's design tables and checks every id against
//! them. Those rules wait for the server to read the tables (M5, #180); what's checked here needs
//! none: the sections, how many values each holds, and the masks that say how many a native
//! reader takes.
use crate::lobby::FACE_MAX;

/// The face version this client build writes, and the only one Fenji passes on (the validator
/// reads older ones differently, and refuses newer).
const VERSION: &str = "v:5";

/// The section 2 layer types, in the validator's order of output, and how many values each
/// entry holds: (type << 16 | id), alpha and colour, then its extras. Not type 7: the create screen
/// has no panel for it, and the client's own writer skips it, though the validator would keep one.
const LAYERS: [(i32, std::ops::RangeInclusive<usize>); 9] = [
    (1, 3..=3),
    (2, 3..=3),
    (3, 3..=4),
    (4, 3..=3),
    (5, 3..=3),
    (6, 3..=5),
    (0, 3..=3),
    (8, 4..=4),
    (9, 4..=4),
];

/// The most section 5 entries a face may hold: one per body part, which the validator checks
/// against the body table; the client's default face has 53.
const BODY_MAX: usize = 256;

/// Whether `face` has the shape the client's shared validator writes, for a character of
/// `vocation` (#391): `v:5`, then sections `1` to `7`, each at most once, separated by `;`, with
/// at most one `;` at the end, as the client writes it.
/// - 2 (the layers), 3 (the eyes) and 4 (the hair) are there, as the validator always writes them;
///   7 (the fox's ears and tail) only for vocation 7, the Fox Mage, whose skeleton is 3.
/// - Every value is a decimal integer within 32 bits, an optional `-` then digits.
/// - 1, the sliders: values, then two of the validator's own, then two masks, of bits 0–30 and
///   0–29, which together have as many bits as there are values.
/// - 2: `|`-separated layers, each of a type the validator knows, with that type's count of values,
///   no type twice.
/// - 3: five values. 4: a mask of bits 0–4, then as many values as it has bits. 5: up to
///   [`BODY_MAX`] values. 6: one value. 7: empty, or up to two `|`-separated pairs, an ear (part 0)
///   and a tail (part 1).
///
/// Ids, masks and packed values aren't negative, but for the eyes' packed saturation and lightness,
/// which the shop clamps to −50..50. Values the validator clamps to the design tables are only
/// checked to be integers. The shape takes some faces the client never writes, such as a hair mask
/// without its base or a layer of id 0; none of those can mislead a reader of the shape, and the
/// design tables' rules (M5, #180) will refuse them.
pub fn valid(face: &str, vocation: i64) -> bool {
    if face.len() > FACE_MAX || !face.is_ascii() {
        return false;
    }
    let body = face.strip_suffix(';').unwrap_or(face);
    let mut sections = body.split(';');
    if sections.next() != Some(VERSION) {
        return false;
    }
    let mut seen = [false; 8];
    for section in sections {
        let Some((key, value)) = section.split_once(':') else {
            return false;
        };
        let key = match key {
            "1" => 1,
            "2" => 2,
            "3" => 3,
            "4" => 4,
            "5" => 5,
            "6" => 6,
            "7" if vocation == 7 => 7,
            _ => return false,
        };
        if std::mem::replace(&mut seen[key], true) {
            return false;
        }
        let ok = match key {
            1 => sliders(value),
            2 => layers(value),
            3 => ints(value).is_some_and(|v| v.len() == 5 && non_negative(&v, [0, 1, 2])),
            4 => hair(value),
            5 => ints(value).is_some_and(|v| v.len() <= BODY_MAX && v.iter().all(|&x| x >= 0)),
            6 => ints(value).is_some_and(|v| v.len() == 1 && v[0] >= 0),
            _ => fox(value),
        };
        if !ok {
            return false;
        }
    }
    seen[2] && seen[3] && seen[4]
}

/// A `,`-separated list of decimal integers within 32 bits: an optional `-`, then 1 to 10 digits.
fn ints(list: &str) -> Option<Vec<i32>> {
    list.split(',')
        .map(|n| {
            let digits = n.strip_prefix('-').unwrap_or(n);
            let plain =
                (1..=10).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
            plain.then(|| n.parse().ok()).flatten()
        })
        .collect()
}

/// Whether `values` at each of `at` is at least 0.
fn non_negative<const N: usize>(values: &[i32], at: [usize; N]) -> bool {
    at.iter().all(|&i| values.get(i).is_some_and(|&v| v >= 0))
}

/// Section 1 (S15 `0.13`): the sliders' values, two values the validator passes through, then
/// the masks of sliders 0–30 and 31–60, whose bits count the values.
fn sliders(value: &str) -> bool {
    let Some(v) = ints(value) else {
        return false;
    };
    let Some(n) = v.len().checked_sub(4) else {
        return false;
    };
    let (low, high) = (v[n + 2], v[n + 3]);
    low >= 0
        && (0..=0x3FFF_FFFF).contains(&high)
        && (low.count_ones() + high.count_ones()) as usize == n
}

/// Section 2 (S15 `0.26`): layers, each (type << 16 | id), alpha and colour, then its type's
/// extras ([`LAYERS`]), no type twice.
fn layers(value: &str) -> bool {
    let mut types = Vec::new();
    for layer in value.split('|') {
        let Some(v) = ints(layer) else {
            return false;
        };
        let Some(&head) = v.first().filter(|&&h| h >= 0) else {
            return false;
        };
        let kind = head >> 16;
        let Some((_, counts)) = LAYERS.iter().find(|(t, _)| *t == kind) else {
            return false;
        };
        if !counts.contains(&v.len()) || !non_negative(&v, [1, 2]) || types.contains(&kind) {
            return false;
        }
        types.push(kind);
    }
    true
}

/// Section 4 (S15 `0.33`): a mask of the base hair, fringe, decoration, hair colour and
/// decoration colour (bits 0–4), then one value, an id plus one, for each of its bits.
fn hair(value: &str) -> bool {
    ints(value).is_some_and(|v| {
        let mask = v[0];
        (0..=31).contains(&mask)
            && v.len() == 1 + mask.count_ones() as usize
            && v.iter().all(|&x| x >= 0)
    })
}

/// Section 7 (S15 `0.25`): empty, or up to two pairs of (part << 8 | model) and a texture, an ear
/// (part 0) and a tail (part 1), each once.
fn fox(value: &str) -> bool {
    if value.is_empty() {
        return true;
    }
    let mut parts = Vec::new();
    for pair in value.split('|') {
        let ok = ints(pair).is_some_and(|v| {
            let part = v[0] >> 8;
            v.len() == 2 && v[0] >= 0 && v[1] >= 0 && part <= 1 && !parts.contains(&part) && {
                parts.push(part);
                true
            }
        });
        if !ok {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face of every section, in the shape the check takes, for vocation 7, its digits ours.
    const EVERY: &str = "v:5;1:120,-340,560,7,9,11,0;2:65543,255,16777215|196613,200,12618100,500;\
                         3:2,6553650,5243780,0,3276850;4:13,121,8,25;5:66536,132572,197608;6:12;\
                         7:1,1|300,502;";

    #[test]
    fn the_widest_face_the_rules_take_fits_within_the_limit() {
        // Every section at its widest, its widest values: 4,088 bytes. So the stored face, the
        // select list and the world's enter stay bounded, and the length check refuses nothing
        // the shape takes; it only stops reading a longer string early.
        let (max, min) = ("2147483647", "-1000000000");
        let sliders = format!("{}2147483647,1073741823", format!("{min},").repeat(63));
        let layers = [
            65535, 131071, 196607, 262143, 327679, 393215, 458751, 589823, 655359,
        ]
        .map(|head: i32| {
            let extras = match head >> 16 {
                3 | 8 | 9 => 1,
                6 => 2,
                _ => 0,
            };
            format!("{head},{max},{max}{}", format!(",{min}").repeat(extras))
        })
        .join("|");
        let face = format!(
            "v:5;1:{sliders};2:{layers};3:{max},{max},{max},{min},{min};\
             4:31,{max},{max},{max},{max},{max};5:{};6:{max};7:255,{max}|511,{max};",
            vec![max; 256].join(","),
        );
        assert!(valid(&face, 7), "{face}");
        assert_eq!(face.len(), 4_088);
        assert!(face.len() <= FACE_MAX);
    }

    #[test]
    fn the_clients_shape_is_valid() {
        assert!(valid(EVERY, 7));
        // Without its trailing `;`, and with only what the validator always writes.
        assert!(valid(EVERY.strip_suffix(';').unwrap(), 7));
        assert!(valid(
            "v:5;2:65543,255,16777215;3:2,6553650,5243780,0,0;4:9,121,25",
            0
        ));
        // A Fox Mage's section 7 may be empty, and hold its pairs in either order.
        assert!(valid("v:5;2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2;7:", 7));
        assert!(valid(
            "v:5;2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2;7:300,502|1,1",
            7
        ));
    }

    #[test]
    fn the_whole_string_is_the_validators_or_none() {
        let base = "2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2";
        for bad in [
            String::new(),
            base.to_owned(),
            format!("v:4;{base}"),
            format!("v:6;{base}"),
            format!("v:05;{base}"),
            format!("V:5;{base}"),
            format!("v:5;{base};;"),
            format!("v:5;;{base}"),
            format!("v:5; {base}"),
            format!("v:5;{base};8:1"),
            format!("v:5;{base};x"),
            format!("v:5;{base};6:1;6:2"),
            format!("v:5;{base};2:65543,255,16777215"),
            format!("v:5;{base};7:1,1"),
            format!("v:5;{base};6:é"),
            format!("v:5;{base};5:{}", "1,".repeat(2_100)),
        ] {
            assert!(!valid(&bad, 0), "{bad:?}");
        }
        // 2, 3 and 4 are always there.
        for missing in [
            "3:2,1,1,0,0;4:9,1,2",
            "2:65543,255,16777215;4:9,1,2",
            "2:65543,255,16777215;3:2,1,1,0,0",
        ] {
            assert!(!valid(&format!("v:5;{missing}"), 0), "{missing}");
        }
    }

    #[test]
    fn every_value_is_a_decimal_integer_within_32_bits() {
        let face = |six: &str| format!("v:5;2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2;6:{six}");
        assert!(valid(&face("2147483647"), 0));
        for bad in [
            "2147483648",
            "+1",
            "1 ",
            " 1",
            "0x10",
            "1.5",
            "",
            "-",
            "--1",
            "1e3",
            "99999999999",
            "00000000001",
            "1,",
        ] {
            assert!(!valid(&face(bad), 0), "{bad:?}");
        }
        // Where the validator writes no negative value: an id, a mask, a packed value.
        assert!(!valid(&face("-1"), 0));
        // But the eyes' packed saturation and lightness may be: the shop clamps them to −50..50.
        assert!(valid(
            "v:5;2:65543,255,16777215;3:2,1,1,0,-3276850;4:9,1,2",
            0
        ));
        assert!(!valid("v:5;2:65543,255,16777215;3:2,1,-1,0,0;4:9,1,2", 0));
        assert!(
            valid(
                "v:5;1:-5,0,0,1,0;2:65543,255,16777215;3:2,1,1,-7,0;4:9,1,2",
                0
            ),
            "a slider and a hue may be"
        );
    }

    #[test]
    fn the_sliders_masks_count_their_values() {
        let face = |one: &str| format!("v:5;1:{one};2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2");
        assert!(valid(&face("0,0,0,0"), 0), "no slider");
        // A value, the validator's two, then the masks: one bit for one value.
        assert!(valid(&face("5,0,0,1,0"), 0));
        assert!(valid(&face("5,6,0,0,3,0"), 0), "two bits, two values");
        assert!(!valid(&face("5,6,0,0,1,0"), 0), "one bit, two values");
        assert!(!valid(&face("5,0,0,3,0"), 0), "two bits, one value");
        assert!(valid(&face("5,0,0,0,1"), 0), "the second mask counts too");
        assert!(!valid(&face("0,0,0"), 0), "fewer than four");
        assert!(!valid(&face("0,0,-1,0"), 0), "a negative mask");
        // i32::MIN has one bit set, so its count fits one value: the sign is what refuses it.
        assert!(
            !valid(&face("5,0,0,-2147483648,0"), 0),
            "the first mask's sign"
        );
        // Bits 0–29 of the second mask: 61 sliders in all.
        assert!(valid(
            &face(&format!("{}0,0,2147483647,1073741823", "1,".repeat(61))),
            0
        ));
        assert!(!valid(
            &face(&format!("{}0,0,2147483647,2147483647", "1,".repeat(62))),
            0
        ));
    }

    #[test]
    fn layers_are_of_a_known_type_with_its_count_and_none_twice() {
        let face = |two: &str| format!("v:5;2:{two};3:2,1,1,0,0;4:9,1,2");
        // Types 0 to 6, 8 and 9 (type << 16 | id).
        for (head, values) in [
            (7, "255,1"),
            (65543, "255,1"),
            (131079, "255,1"),
            (196615, "255,1,5"),
            (262151, "255,1"),
            (327687, "255,1"),
            (393223, "255,25700,40,1"),
            (524295, "255,1,1"),
            (589831, "255,1,1"),
        ] {
            assert!(valid(&face(&format!("{head},{values}")), 0), "{head}");
        }
        for bad in [
            "458759,255,1",            // type 7
            "655367,255,1",            // type 10
            "65543,255",               // too few
            "65543,255,1,1",           // a brow has no extras
            "524295,255,1",            // type 8 has its spacing
            "393223,255,1,1,1,1",      // type 6 has at most two extras
            "196615,255,1,5,6",        // type 3 has at most one
            "65543,255,1|65544,255,1", // type 1 twice
            "-1,255,1",
            "65543,-1,1",
            "65543,255,-1",
            "",
            "65543,255,1|",
        ] {
            assert!(!valid(&face(bad), 0), "{bad:?}");
        }
    }

    #[test]
    fn the_hair_mask_counts_its_values() {
        let face = |four: &str| format!("v:5;2:65543,255,16777215;3:2,1,1,0,0;4:{four}");
        assert!(valid(&face("9,121,25"), 0));
        assert!(valid(&face("31,1,2,3,4,5"), 0));
        assert!(valid(&face("0"), 0));
        for bad in ["9,121", "9,121,25,1", "32,1", "89,1,2,3", "-1", "9,-1,2"] {
            assert!(!valid(&face(bad), 0), "{bad:?}");
        }
    }

    #[test]
    fn the_eyes_body_and_skin_have_their_counts() {
        let face = |rest: &str| format!("v:5;2:65543,255,16777215;4:9,1,2;{rest}");
        assert!(valid(&face("3:2,1,1,0,0"), 0));
        assert!(!valid(&face("3:2,1,1,0"), 0));
        assert!(!valid(&face("3:2,1,1,0,0,0"), 0));
        assert!(!valid(&face("3:-2,1,1,0,0"), 0));
        let eyes = "3:2,1,1,0,0;";
        assert!(valid(
            &face(&format!("{eyes}5:{}", "66536,".repeat(255) + "66536")),
            0
        ));
        assert!(!valid(
            &face(&format!("{eyes}5:{}", "66536,".repeat(256) + "66536")),
            0
        ));
        assert!(!valid(&face(&format!("{eyes}5:-1")), 0));
        assert!(!valid(&face(&format!("{eyes}6:1,2")), 0));
    }

    #[test]
    fn a_fox_has_one_ear_and_one_tail_and_no_one_else_has_either() {
        let face = |seven: &str| format!("v:5;2:65543,255,16777215;3:2,1,1,0,0;4:9,1,2;7:{seven}");
        assert!(valid(&face("1,1|300,502"), 7));
        assert!(valid(&face("1,1"), 7));
        for bad in [
            "1,1|2,2",
            "300,1|301,2",
            "512,1",
            "1,1|300,502|1,1",
            "1",
            "1,1,1",
            "-1,1",
            "1,-1",
        ] {
            assert!(!valid(&face(bad), 7), "{bad:?}");
        }
        for vocation in [0, 6, 9] {
            assert!(!valid(&face("1,1|300,502"), vocation), "{vocation}");
        }
    }
}
