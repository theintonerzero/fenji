//! The inner message header's type byte, by direction (docs/protocol/opcodes.md). Values not in the
//! client's enums decode to `Unknown`, never an error.

/// `STC_PROTOCOL_*`, server to client: 0–29 in the enum's order (FUN_140222E00's switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stc {
    Script,
    Close,
    TimeSync,
    TransLimitLv,
    DebugPos,
    GmChannel,
    SyncIdEnter,
    SyncEnter,
    SyncEnterFinish,
    SyncLeave,
    SyncUpdate,
    SyncUpdateFilter,
    Move,
    MoveDir,
    MovePos,
    MoveCombat,
    MoveJumpPos,
    MoveStop,
    MoveYaw,
    MoveFinishCharging,
    MoveSyncSvr,
    MoveBaseProp,
    MoveSyncPathFindTask,
    SyncPropForce,
    ActorHmapMaskRefresh,
    OverallHmapMaskRefresh,
    MoveEnterSyncState,
    MovePassive,
    MoveSurround,
    MoveFormular,
    Unknown(u8),
}

const STC: [Stc; 30] = [
    Stc::Script,
    Stc::Close,
    Stc::TimeSync,
    Stc::TransLimitLv,
    Stc::DebugPos,
    Stc::GmChannel,
    Stc::SyncIdEnter,
    Stc::SyncEnter,
    Stc::SyncEnterFinish,
    Stc::SyncLeave,
    Stc::SyncUpdate,
    Stc::SyncUpdateFilter,
    Stc::Move,
    Stc::MoveDir,
    Stc::MovePos,
    Stc::MoveCombat,
    Stc::MoveJumpPos,
    Stc::MoveStop,
    Stc::MoveYaw,
    Stc::MoveFinishCharging,
    Stc::MoveSyncSvr,
    Stc::MoveBaseProp,
    Stc::MoveSyncPathFindTask,
    Stc::SyncPropForce,
    Stc::ActorHmapMaskRefresh,
    Stc::OverallHmapMaskRefresh,
    Stc::MoveEnterSyncState,
    Stc::MovePassive,
    Stc::MoveSurround,
    Stc::MoveFormular,
];

impl Stc {
    pub fn from_u8(v: u8) -> Self {
        STC.get(usize::from(v)).copied().unwrap_or(Self::Unknown(v))
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::Unknown(v) => v,
            known => STC.iter().position(|&s| s == known).expect("in the table") as u8,
        }
    }
}

/// `CTS_PROTOCOL_*`, client to server: `GATE_SCRIPT` is −1 on the wire (0xFF), the rest count
/// from 0 in the enum's order (the send methods' type constants, opcodes.md O.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cts {
    GateScript,
    Script,
    GmChannel,
    TimeSync,
    ClientEnter,
    ClientLeave,
    SyncFilterSet,
    QueryActorPos,
    QueryActorMoveCmd,
    MoveDir,
    MoveCombat,
    MoveFinishCharge,
    MoveDebug,
    Unknown(u8),
}

const CTS: [Cts; 12] = [
    Cts::Script,
    Cts::GmChannel,
    Cts::TimeSync,
    Cts::ClientEnter,
    Cts::ClientLeave,
    Cts::SyncFilterSet,
    Cts::QueryActorPos,
    Cts::QueryActorMoveCmd,
    Cts::MoveDir,
    Cts::MoveCombat,
    Cts::MoveFinishCharge,
    Cts::MoveDebug,
];

impl Cts {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0xff => Self::GateScript,
            _ => CTS.get(usize::from(v)).copied().unwrap_or(Self::Unknown(v)),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            Self::GateScript => 0xff,
            Self::Unknown(v) => v,
            known => CTS.iter().position(|&c| c == known).expect("in the table") as u8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_opcodes() {
        // opcodes.md O.2: send_gateway 0xFF, send 0, send_time_sync 2, send_enter 3,
        // send_leave 4, send_sync_filter 5.
        assert_eq!(Cts::from_u8(0xff), Cts::GateScript);
        assert_eq!(Cts::from_u8(0), Cts::Script);
        assert_eq!(Cts::from_u8(2), Cts::TimeSync);
        assert_eq!(Cts::from_u8(3), Cts::ClientEnter);
        assert_eq!(Cts::from_u8(4), Cts::ClientLeave);
        assert_eq!(Cts::from_u8(5), Cts::SyncFilterSet);
        assert_eq!(Cts::from_u8(11), Cts::MoveDebug);
        assert_eq!(Cts::from_u8(12), Cts::Unknown(12));
        assert_eq!(Cts::GateScript.to_u8(), 0xff);
    }

    #[test]
    fn server_opcodes() {
        // opcodes.md O.1: the switch over 0–29 in FUN_140222E00, in enum order.
        assert_eq!(Stc::from_u8(0), Stc::Script);
        assert_eq!(Stc::from_u8(1), Stc::Close);
        assert_eq!(Stc::from_u8(2), Stc::TimeSync);
        assert_eq!(Stc::from_u8(29), Stc::MoveFormular);
        assert_eq!(Stc::from_u8(30), Stc::Unknown(30));
        assert_eq!(Stc::Script.to_u8(), 0);
    }

    #[test]
    fn every_value_round_trips() {
        for v in 0..=255u8 {
            assert_eq!(Cts::from_u8(v).to_u8(), v, "cts {v}");
            assert_eq!(Stc::from_u8(v).to_u8(), v, "stc {v}");
        }
    }
}
