//! Intercom state/event/error enums. Spec: §3.5. change 08.
//! Signatures kept 1:1 with the technical design so change 09+ can copy
//! trait signatures without reshuffling types.

#![allow(dead_code)]

use crate::services::network::NetError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntercomState {
    Idle,
    Hosting(HostPhase),
    Joining(JoinPhase),
    Grouped(VoiceState),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPhase {
    Discovering,
    CollectingPeers,
    Frozen,
    SwitchingChannel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinPhase {
    Searching,
    Requesting,
    WaitingConfirm,
    SwitchingChannel,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle = 0,
    Talking = 1,
    Listening = 2,
    ChannelBusy = 3,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntercomMode {
    Clear = 0,
    Free = 1,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEffect {
    Normal = 0,
    PitchUp = 1,
    PitchDown = 2,
}

impl VoiceEffect {
    pub fn to_u8(&self) -> u8 {
        *self as u8
    }
    pub fn from_u8(v: u8) -> Option<VoiceEffect> {
        match v {
            0 => Some(VoiceEffect::Normal),
            1 => Some(VoiceEffect::PitchUp),
            2 => Some(VoiceEffect::PitchDown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum IntercomEvent {
    StateChanged(IntercomState),
    PeerOnline(u8, u8),
    PeerOffline(u8),
    VoiceActive(u8),
    ChannelBusy,
    PttReady,
    PairFailed(String),
    /// A host beacon was discovered/updated during search; payload is the
    /// current discovered-host count (UI reads the list from the coordinator).
    HostDiscovered(usize),
    /// Local device's join request was accepted by the host.
    JoinAccepted,
    /// Local device's join request was rejected; payload is a display reason.
    JoinRejected(String),
    /// Three-phase pairing completed and the encrypted mesh is up.
    GroupFormed,
    /// Local device left the group (NVS cleared, peers removed).
    LeftGroup,
    /// A peer's TALK_STATE changed (talking = true on start, false on end).
    TalkState { sender_id: u8, talking: bool },
    /// The peer roster changed (add/remove/online/offline) — UI should refresh.
    PeerListChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntercomError {
    Busy,
    NotGrouped,
    ChannelBusy,
    InvalidState,
    Net(NetError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_effect_roundtrip() {
        assert_eq!(VoiceEffect::PitchUp.to_u8(), 1);
        assert_eq!(VoiceEffect::from_u8(1), Some(VoiceEffect::PitchUp));
        assert_eq!(VoiceEffect::from_u8(9), None);
    }

    #[test]
    fn state_variants_constructible() {
        let _ = IntercomState::Hosting(HostPhase::Frozen);
        let _ = IntercomState::Joining(JoinPhase::WaitingConfirm);
        let _ = IntercomState::Grouped(VoiceState::Talking);
    }

    #[test]
    fn debug_format_contains_variant_names() {
        let s = format!("{:?}", IntercomState::Hosting(HostPhase::Frozen));
        assert!(s.contains("Hosting") && s.contains("Frozen"));
    }
}
