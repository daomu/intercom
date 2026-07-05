//! Intercom app UI (change 13). Spec: §3.7, §15, §13.2.
//!
//! Pure-logic intercom app state machine. Maps PTT touch/button events to
//! IntercomService calls, tracks UI state (Idle / Listening / PttArming /
//! PttActive), displays peer cards with signal bars + voice-active indicator.
//! Slint rendering lands in change 17; this module hosts the testable state.

#![allow(dead_code)]

use std::fmt;

use crate::intercom::state::{IntercomMode, VoiceState};
use crate::intercom::voice::{VoiceAction, VoiceEvent, VoicePttMachine};
use crate::services::input::InputEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntercomUiState {
    Idle,
    Listening,
    PttArming,
    PttActive,
    ChannelBusy,
}

/// Peer card display data (PRD §13.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCard {
    pub sender_id: u8,
    pub name: String,
    pub online: bool,
    pub rssi_bars: u8,
    pub voice_active: bool,
}

/// Outcome of an intercom-app event dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntercomAppOutcome {
    pub new_state: IntercomUiState,
    pub voice_actions: Vec<VoiceAction>,
    pub refresh_peer_cards: bool,
}

pub struct IntercomApp {
    ui_state: IntercomUiState,
    ptt: VoicePttMachine,
    mode: IntercomMode,
    peers: Vec<PeerCard>,
}

impl fmt::Debug for IntercomApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntercomApp")
            .field("ui_state", &self.ui_state)
            .field("mode", &self.mode)
            .field("peers", &self.peers.len())
            .finish_non_exhaustive()
    }
}

impl IntercomApp {
    pub fn new(mode: IntercomMode) -> Self {
        Self {
            ui_state: IntercomUiState::Idle,
            ptt: VoicePttMachine::new(mode),
            mode,
            peers: Vec::new(),
        }
    }

    pub fn ui_state(&self) -> IntercomUiState {
        self.ui_state
    }

    pub fn mode(&self) -> IntercomMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: IntercomMode) {
        self.mode = mode;
        self.ptt.set_mode(mode);
    }

    pub fn peers(&self) -> &[PeerCard] {
        &self.peers
    }

    /// Update peer cards from heartbeat tracker data.
    pub fn update_peers(&mut self, peers: Vec<PeerCard>) {
        self.peers = peers;
    }

    /// Translate UI input event into VoiceEvent for the PTT machine.
    fn input_to_voice_event(ev: &InputEvent) -> Option<VoiceEvent> {
        match ev {
            InputEvent::BootPress { screen_was_off } => Some(VoiceEvent::PttPress {
                screen_was_off: *screen_was_off,
            }),
            InputEvent::BootRelease => Some(VoiceEvent::PttRelease),
            _ => None,
        }
    }

    /// Dispatch an input event. Returns outcome with voice actions to
    /// execute against IntercomService / AudioService.
    pub fn dispatch(&mut self, ev: &InputEvent) -> IntercomAppOutcome {
        let mut outcome = IntercomAppOutcome {
            new_state: self.ui_state,
            voice_actions: Vec::new(),
            refresh_peer_cards: false,
        };

        // PTT-relevant events go to the voice state machine.
        if let Some(ve) = Self::input_to_voice_event(ev) {
            let voice_outcome = self.ptt.handle(ve);
            outcome.voice_actions = voice_outcome.actions.clone();
            self.ui_state = match voice_outcome.new_state {
                VoiceState::Idle => IntercomUiState::Idle,
                VoiceState::Talking => IntercomUiState::PttActive,
                VoiceState::Listening => IntercomUiState::Listening,
                VoiceState::ChannelBusy => IntercomUiState::ChannelBusy,
            };
            outcome.new_state = self.ui_state;
            return outcome;
        }

        // Touch events on peer cards could trigger "view peer details" etc.
        // For now: any touch in intercom app refreshes peer cards.
        if matches!(ev, InputEvent::Touch(_)) {
            outcome.refresh_peer_cards = true;
        }
        outcome
    }

    /// Called when a remote peer's voice state changes (via TALK_STATE packet).
    pub fn on_peer_voice_state(&mut self, sender_id: u8, talking: bool) {
        if let Some(p) = self.peers.iter_mut().find(|p| p.sender_id == sender_id) {
            p.voice_active = talking;
        }
        // If a peer started talking and we're idle, transition UI to Listening.
        if talking && self.ui_state == IntercomUiState::Idle {
            self.ui_state = IntercomUiState::Listening;
        } else if !talking {
            // If the talking peer stopped and we're listening with no other
            // active talkers, return to Idle.
            let any_talking = self.peers.iter().any(|p| p.voice_active);
            if !any_talking && self.ui_state == IntercomUiState::Listening {
                self.ui_state = IntercomUiState::Idle;
            }
        }
    }

    /// Called periodically to advance arbitration state. The caller emits
    /// `ArbitrationTimeout` after 50ms or `TalkStateReceived` when a peer's
    /// TALK_STATE arrives.
    pub fn dispatch_voice(&mut self, ev: VoiceEvent) -> IntercomAppOutcome {
        let voice_outcome = self.ptt.handle(ev);
        self.ui_state = match voice_outcome.new_state {
            VoiceState::Idle => IntercomUiState::Idle,
            VoiceState::Talking => IntercomUiState::PttActive,
            VoiceState::Listening => IntercomUiState::Listening,
            VoiceState::ChannelBusy => IntercomUiState::ChannelBusy,
        };
        IntercomAppOutcome {
            new_state: self.ui_state,
            voice_actions: voice_outcome.actions,
            refresh_peer_cards: false,
        }
    }
}

// ---- Trait shim ---------------------------------------------------------

pub trait IntercomAppTrait: Send + Sync + fmt::Debug {
    fn current_state(&self) -> IntercomUiState;
    fn on_ptt_press(&self);
    fn on_ptt_release(&self);
    fn on_incoming_voice(&self, src_id: u16);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::input::InputEvent;

    fn mk() -> IntercomApp {
        IntercomApp::new(IntercomMode::Clear)
    }

    fn mk_with_peers() -> IntercomApp {
        let mut a = mk();
        a.update_peers(vec![
            PeerCard { sender_id: 1, name: "Alice".into(), online: true, rssi_bars: 4, voice_active: false },
            PeerCard { sender_id: 2, name: "Bob".into(), online: true, rssi_bars: 3, voice_active: false },
        ]);
        a
    }

    #[test]
    fn free_mode_ptt_press_goes_active() {
        let mut a = IntercomApp::new(IntercomMode::Free);
        let o = a.dispatch(&InputEvent::BootPress { screen_was_off: false });
        assert_eq!(o.new_state, IntercomUiState::PttActive);
        assert!(o.voice_actions.contains(&VoiceAction::StartCapture));
    }

    #[test]
    fn clear_mode_ptt_press_enters_arming() {
        let mut a = mk();
        let o = a.dispatch(&InputEvent::BootPress { screen_was_off: false });
        // clear-mode: stays Idle (waiting for arbitration).
        assert_eq!(o.new_state, IntercomUiState::Idle);
        assert!(o.voice_actions.contains(&VoiceAction::SendTalkState { action: 1 }));
        // Arbitration timeout → PttActive.
        let o = a.dispatch_voice(VoiceEvent::ArbitrationTimeout);
        assert_eq!(o.new_state, IntercomUiState::PttActive);
    }

    #[test]
    fn ptt_release_returns_to_idle() {
        let mut a = IntercomApp::new(IntercomMode::Free);
        a.dispatch(&InputEvent::BootPress { screen_was_off: false });
        let o = a.dispatch(&InputEvent::BootRelease);
        assert_eq!(o.new_state, IntercomUiState::Idle);
        assert!(o.voice_actions.contains(&VoiceAction::StopCapture));
    }

    #[test]
    fn peer_starts_talking_transitions_to_listening() {
        let mut a = mk_with_peers();
        assert_eq!(a.ui_state(), IntercomUiState::Idle);
        a.on_peer_voice_state(1, true);
        assert_eq!(a.ui_state(), IntercomUiState::Listening);
        assert!(a.peers()[0].voice_active);
    }

    #[test]
    fn peer_stops_talking_returns_to_idle() {
        let mut a = mk_with_peers();
        a.on_peer_voice_state(1, true);
        a.on_peer_voice_state(1, false);
        assert_eq!(a.ui_state(), IntercomUiState::Idle);
        assert!(!a.peers()[0].voice_active);
    }

    #[test]
    fn multiple_peers_talking_stays_listening() {
        let mut a = mk_with_peers();
        a.on_peer_voice_state(1, true);
        a.on_peer_voice_state(2, true);
        a.on_peer_voice_state(1, false);
        // Bob still talking.
        assert_eq!(a.ui_state(), IntercomUiState::Listening);
    }

    #[test]
    fn touch_event_refreshes_peer_cards() {
        let mut a = mk_with_peers();
        use crate::hal::touch::TouchEvent;
        let o = a.dispatch(&InputEvent::Touch(TouchEvent::Down { x: 5, y: 5 }));
        assert!(o.refresh_peer_cards);
    }
}
