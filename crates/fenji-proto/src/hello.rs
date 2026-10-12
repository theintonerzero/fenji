//! The server's 64-byte hello (FUN_14029C330; docs/protocol/transport.md §2). The client reads it
//! as 16 little-endian words. It accepts it when `w[(w1 % 14) + 2] == (K | 0xC0000000) ^ w1 ^ w0`,
//! then picks its receive variant from `w0 % 27` and its send variant from `w1 % 27`. The hello is
//! sent in clear and the variant keys are folded from it, so it needs to be unique, not secret.
use crate::variant::VARIANT_COUNT;
use crate::{Error, Result};

/// The hello's length: 16 words, outside any frame (transport.md 2.1).
pub const HELLO_LEN: usize = 64;
/// `net_client_create`'s integer argument (net_manager.luac `0.20`), kept at ctx+0x50 by
/// FUN_14029A900 (docs/protocol/login.md 2.6–2.7). A client whose `K` differs closes with
/// `CLOSED_VERSION`, so it acts as a protocol version (transport.md 2.4).
pub const CLIENT_MAGIC: u32 = 0xe62f_7b30;

/// The variant for each direction: `down` (server to client) is `w0 % 27`, `up` is `w1 % 27`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Variants {
    pub down: u32,
    pub up: u32,
}

fn word(h: &[u8; HELLO_LEN], i: usize) -> u32 {
    u32::from_le_bytes([h[4 * i], h[4 * i + 1], h[4 * i + 2], h[4 * i + 3]])
}

fn set_word(h: &mut [u8; HELLO_LEN], i: usize, v: u32) {
    h[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
}

/// The largest multiple of 27 that fits, so that `seed * 27 + v` can't overflow.
const SEEDS: u32 = u32::MAX / VARIANT_COUNT;

/// Builds a hello from `fill` (random bytes) that chooses `v` and passes the client's check.
pub fn build(fill: [u8; HELLO_LEN], v: Variants, magic: u32) -> Result<[u8; HELLO_LEN]> {
    if v.down >= VARIANT_COUNT || v.up >= VARIANT_COUNT {
        return Err(Error::BadHello(format!(
            "variants {v:?} are not all below 27"
        )));
    }
    let mut h = fill;
    let w0 = (word(&h, 0) % SEEDS) * VARIANT_COUNT + v.down;
    let w1 = (word(&h, 1) % SEEDS) * VARIANT_COUNT + v.up;
    set_word(&mut h, 0, w0);
    set_word(&mut h, 1, w1);
    set_word(
        &mut h,
        (w1 % 14 + 2) as usize,
        (magic | 0xc000_0000) ^ w1 ^ w0,
    );
    Ok(h)
}

/// The client's check: the variants if the hello passes.
pub fn check(h: &[u8; HELLO_LEN], magic: u32) -> Result<Variants> {
    let (w0, w1) = (word(h, 0), word(h, 1));
    let idx = (w1 % 14 + 2) as usize;
    if word(h, idx) != (magic | 0xc000_0000) ^ w1 ^ w0 {
        return Err(Error::BadHello(format!("word {idx} fails the check")));
    }
    Ok(Variants {
        down: w0 % VARIANT_COUNT,
        up: w1 % VARIANT_COUNT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(h: &[u8; 64]) -> [u32; 16] {
        core::array::from_fn(|i| u32::from_le_bytes(h[4 * i..4 * i + 4].try_into().unwrap()))
    }

    #[test]
    fn built_hello_passes_the_clients_check() {
        for seed in 0..=255u8 {
            let fill = core::array::from_fn(|i| (i as u8).wrapping_mul(37) ^ seed);
            let v = Variants {
                down: u32::from(seed) % 27,
                up: (u32::from(seed) * 7) % 27,
            };
            let h = build(fill, v, CLIENT_MAGIC).unwrap();
            assert_eq!(check(&h, CLIENT_MAGIC), Ok(v), "seed {seed}");
        }
    }

    #[test]
    fn check_is_the_clients_rule() {
        // F1: w[(w1 % 14) + 2] == (K | 0xC0000000) ^ w1 ^ w0, with w0 = 3 and w1 = 3.
        let mut h = [0u8; 64];
        h[0..4].copy_from_slice(&3u32.to_le_bytes());
        h[4..8].copy_from_slice(&3u32.to_le_bytes());
        let idx = 5; // (w1 % 14) + 2 with w1 = 3
        h[4 * idx..4 * idx + 4].copy_from_slice(&CLIENT_MAGIC.to_le_bytes()); // K ^ 3 ^ 3
        assert_eq!(check(&h, CLIENT_MAGIC), Ok(Variants { down: 3, up: 3 }));
        h[4 * idx] ^= 1;
        assert!(matches!(check(&h, CLIENT_MAGIC), Err(Error::BadHello(_))));
    }

    #[test]
    fn build_keeps_the_fill_where_the_rule_allows() {
        let fill = [0xa5; 64];
        let h = build(fill, Variants { down: 3, up: 3 }, CLIENT_MAGIC).unwrap();
        let w = words(&h);
        let idx = (w[1] % 14 + 2) as usize;
        for (i, &x) in w.iter().enumerate().skip(2) {
            if i != idx {
                assert_eq!(x, 0xa5a5_a5a5, "word {i}");
            }
        }
    }

    #[test]
    fn build_matches_a_hand_built_hello() {
        // Fill words 0 and 1 are 0 and 1; down 3, up 5: w0 = 0 * 27 + 3 = 3, w1 = 1 * 27 + 5 = 32,
        // idx = 32 % 14 + 2 = 6 (w0 would give 5), w6 = 0xE62F7B30 ^ 32 ^ 3 = 0xE62F7B13.
        let mut fill = [0u8; 64];
        fill[4] = 1;
        let mut want = [0u8; 64];
        want[0] = 3;
        want[4] = 32;
        want[24..28].copy_from_slice(&0xe62f_7b13u32.to_le_bytes());
        let v = Variants { down: 3, up: 5 };
        assert_eq!(build(fill, v, CLIENT_MAGIC).unwrap(), want);
        assert_eq!(check(&want, CLIENT_MAGIC), Ok(v));
    }

    #[test]
    fn variant_out_of_range_is_refused() {
        let r = build([0; 64], Variants { down: 27, up: 0 }, CLIENT_MAGIC);
        assert!(matches!(r, Err(Error::BadHello(_))));
    }
}
