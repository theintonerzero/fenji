//! The stream variants (FUN_14029BA10; docs/protocol/transport.md §3). Each direction runs one of
//! 27 variants: eight LibTomCrypt block ciphers in ECB with a byte-wise tail. Only case 3, AES-128,
//! is implemented; our server always chooses it. The client encrypts what it sends (slot 2) and
//! decrypts what it receives (slot 3).
use aes::Aes128;
use aes::cipher::{BlockCipherDecrypt, BlockCipherEncrypt, KeyInit};

use crate::hello::HELLO_LEN;
use crate::{Error, Result};

pub const VARIANT_COUNT: u32 = 27;
/// Case 3: the hello folded to 16 bytes, then AES (init FUN_140299C50, encrypt FUN_140299B90,
/// decrypt FUN_140299AD0).
pub const AES128: u32 = 3;
/// The AES family's tail constant (FUN_140299B90, FUN_140299AD0).
const AES_TAIL: u8 = 0xc6;

/// The key fold (FUN_14029BE20): `key[i % n] ^= hello[i]` for every hello byte, from zero. This
/// holds for keys of up to 64 bytes; longer keys (RC5, RC6) take a generator instead
/// (transport.md 3.5), which no implemented variant needs.
pub fn fold_key(hello: &[u8; HELLO_LEN], n: usize) -> Vec<u8> {
    let mut key = vec![0u8; n];
    for (i, b) in hello.iter().enumerate() {
        key[i % n] ^= b;
    }
    key
}

/// Byte `j` of an `n`-byte tail is XORed with `(n - j) ^ c`: its own inverse.
fn tail(buf: &mut [u8], c: u8) {
    let n = buf.len();
    for (j, b) in buf.iter_mut().enumerate() {
        *b ^= ((n - j) as u8) ^ c;
    }
}

/// One direction's variant.
pub struct Cipher {
    aes: Aes128,
}

impl Cipher {
    pub fn new(variant: u32, hello: &[u8; HELLO_LEN]) -> Result<Self> {
        if variant != AES128 {
            return Err(Error::Unsupported(variant));
        }
        let aes = Aes128::new_from_slice(&fold_key(hello, 16)).expect("16-byte key");
        Ok(Self { aes })
    }

    /// The client's send transform: whole blocks encrypted, then the tail.
    pub fn encrypt(&self, buf: &mut [u8]) {
        let mut blocks = buf.chunks_exact_mut(16);
        for chunk in &mut blocks {
            let mut b: [u8; 16] = (&*chunk).try_into().expect("16 bytes");
            self.aes.encrypt_block((&mut b).into());
            chunk.copy_from_slice(&b);
        }
        tail(blocks.into_remainder(), AES_TAIL);
    }

    /// The client's receive transform: whole blocks decrypted, then the tail.
    pub fn decrypt(&self, buf: &mut [u8]) {
        let mut blocks = buf.chunks_exact_mut(16);
        for chunk in &mut blocks {
            let mut b: [u8; 16] = (&*chunk).try_into().expect("16 bytes");
            self.aes.decrypt_block((&mut b).into());
            chunk.copy_from_slice(&b);
        }
        tail(blocks.into_remainder(), AES_TAIL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockCipherEncrypt, KeyInit};

    fn hello() -> [u8; 64] {
        core::array::from_fn(|i| i as u8)
    }

    #[test]
    fn fold_xors_the_hello_into_the_key() {
        // F2 (FUN_14029BE20): key[i % 16] ^= hello[i].
        let k = fold_key(&hello(), 16);
        let want: Vec<u8> = (0..16u8)
            .map(|i| i ^ (i + 16) ^ (i + 32) ^ (i + 48))
            .collect();
        assert_eq!(k, want);
        // n = 24: bytes 48..64 fold onto key[0..16] as well.
        let k = fold_key(&hello(), 24);
        let want: Vec<u8> = (0..24u8)
            .map(|i| {
                if i < 16 {
                    i ^ (i + 24) ^ (i + 48)
                } else {
                    i ^ (i + 24)
                }
            })
            .collect();
        assert_eq!(k, want);
    }

    #[test]
    fn aes_blocks_are_plain_aes_128_ecb() {
        let c = Cipher::new(AES128, &hello()).unwrap();
        let key = fold_key(&hello(), 16);
        let aes = aes::Aes128::new_from_slice(&key).unwrap();
        let mut want = [0x11u8; 16];
        aes.encrypt_block((&mut want).into());
        let mut got = [0x11u8; 16];
        c.encrypt(&mut got);
        assert_eq!(got, want);
    }

    #[test]
    fn tail_bytes_xor_count_and_c6() {
        // 3 tail bytes after one block: byte j gets (3 - j) ^ 0xC6.
        let c = Cipher::new(AES128, &hello()).unwrap();
        let mut buf = [0u8; 19];
        c.encrypt(&mut buf);
        assert_eq!(&buf[16..], &[3 ^ 0xc6, 2 ^ 0xc6, 1 ^ 0xc6]);
    }

    #[test]
    fn decrypt_inverts_encrypt_for_all_lengths() {
        let c = Cipher::new(AES128, &hello()).unwrap();
        for len in 0..80 {
            let plain: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            let mut buf = plain.clone();
            c.encrypt(&mut buf);
            if len > 0 {
                assert_ne!(buf, plain, "len {len}");
            }
            c.decrypt(&mut buf);
            assert_eq!(buf, plain, "len {len}");
        }
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// transport.md 2.6's example hello: w0 = w1 = 3, w5 = K, the other words zero.
    fn doc_hello() -> [u8; 64] {
        let mut h = [0u8; 64];
        h[0] = 3;
        h[4] = 3;
        h[20..24].copy_from_slice(&0xe62f_7b30u32.to_le_bytes());
        h
    }

    #[test]
    fn doc_hello_folds_to_the_documented_key() {
        // transport.md 3.6.
        let want = [
            0x03, 0, 0, 0, 0x33, 0x7b, 0x2f, 0xe6, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(fold_key(&doc_hello(), 16), want);
    }

    #[test]
    fn known_answer_one_block_and_a_tail() {
        // Block: `openssl enc -aes-128-ecb -nopad -K 03000000337b2fe60000000000000000` on 00..0f.
        // Tail: 10..14, byte j XOR (5 - j) ^ 0xC6.
        let c = Cipher::new(AES128, &doc_hello()).unwrap();
        let mut buf: Vec<u8> = (0..21).collect();
        c.encrypt(&mut buf);
        assert_eq!(hex(&buf), "a23f7760b7d6dac3955f21a0b62502fed3d3d7d7d3");
    }

    #[test]
    fn known_answer_two_blocks_and_a_tail() {
        // As above, plus openssl on 10..1f; tail 20..22 XOR (3 - j) ^ 0xC6.
        let c = Cipher::new(AES128, &doc_hello()).unwrap();
        let plain: Vec<u8> = (0..35).collect();
        let mut buf = plain.clone();
        c.encrypt(&mut buf);
        assert_eq!(
            hex(&buf),
            "a23f7760b7d6dac3955f21a0b62502fe2c80708867fd848eca7e979bcc2201cae5e5e5"
        );
        c.decrypt(&mut buf);
        assert_eq!(buf, plain);
    }

    #[test]
    fn fips_197_c1_through_the_fold() {
        // A hello of 00..0f followed by zeros folds to the key 000102..0f (FIPS-197 C.1).
        let mut h = [0u8; 64];
        for (i, b) in h.iter_mut().take(16).enumerate() {
            *b = i as u8;
        }
        let c = Cipher::new(AES128, &h).unwrap();
        let mut block: [u8; 16] = core::array::from_fn(|i| (i as u8) * 0x11);
        c.encrypt(&mut block);
        assert_eq!(hex(&block), "69c4e0d86a7b0430d8cdb78070b4c55a");
    }

    #[test]
    fn other_variants_are_unsupported() {
        for v in (0..27).filter(|&v| v != AES128) {
            assert!(matches!(Cipher::new(v, &hello()), Err(Error::Unsupported(n)) if n == v));
        }
    }
}
