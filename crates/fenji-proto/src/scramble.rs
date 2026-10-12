//! The frames' 16-bit length keys (spike-0 4.30; docs/protocol/transport.md §4). Both live in the
//! variant contexts that FUN_14029BA10 clears, so both are 0 once the hello is handled.

/// Server to client: the client's receive key (ctx+0x1480). It steps by 0xD736 after every frame,
/// data or time sync (FUN_14029B850; 0x14029B904, 0x14029B964).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DownKey(pub u16);

impl DownKey {
    pub fn step(&mut self) {
        self.0 = self.0.wrapping_add(0xd736);
    }
}

/// Client to server: the client's send key (ctx+0x2548). A data frame steps it to
/// `(k + 0x28CA) ^ k` (FUN_14029C600), a time-sync frame to `(k + 0xB4FB) ^ k` (FUN_14029A5B0).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct UpKey(pub u16);

impl UpKey {
    pub fn step_data(&mut self) {
        self.0 = self.0.wrapping_add(0x28ca) ^ self.0;
    }

    pub fn step_time_sync(&mut self) {
        self.0 = self.0.wrapping_add(0xb4fb) ^ self.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_key_sequence() {
        let mut k = DownKey::default();
        let mut seen = Vec::new();
        for _ in 0..3 {
            seen.push(k.0);
            k.step();
        }
        // 0, 0xD736, 0xD736 * 2 mod 2^16 = 0xAE6C.
        assert_eq!(seen, [0x0000, 0xd736, 0xae6c]);
    }

    #[test]
    fn up_key_sequences() {
        let mut k = UpKey::default();
        k.step_data(); // (0 + 0x28CA) ^ 0 = 0x28CA
        assert_eq!(k.0, 0x28ca);
        k.step_data(); // (0x28CA + 0x28CA) ^ 0x28CA = 0x5194 ^ 0x28CA = 0x795E
        assert_eq!(k.0, 0x795e);
        k.step_time_sync(); // (0x795E + 0xB4FB) ^ 0x795E = 0x2E59 ^ 0x795E = 0x5707
        assert_eq!(k.0, 0x5707);
    }
}
