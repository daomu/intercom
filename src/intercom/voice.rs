//! Voice PTT state machine (change 10). Spec: §3.5, §11, §14.1/§14.2.
//!
//! Pure-logic state machine. Side effects (start_capture, pa_enable,
//! play_tone, send_talk_state, screen_on) are emitted as `VoiceAction`s so
//! the caller (IntercomService on Task B) can execute them. The arbitration
//! window is event-driven: caller emits `ArbitrationTimeout` after 50ms or
//! `TalkStateReceived{action=1}` when a peer's TALK_STATE arrives — whichever
//! first.

#![allow(dead_code)]

use std::fmt;

use crate::intercom::state::{IntercomMode, VoiceState};

/// Events driving the PTT state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceEvent {
    /// BOOT long-press (≥50ms) or touch-PTT down.
    PttPress { screen_was_off: bool },
    /// BOOT release or touch-PTT up.
    PttRelease,
    /// Received a peer's TALK_STATE packet.
    TalkStateReceived { action: u8, from_sender: u8 },
    /// 50ms arbitration window elapsed with no conflicting TALK_STATE.
    ArbitrationTimeout,
    /// Busy tone playback finished (auto-recover from ChannelBusy → Idle).
    BusyToneDone,
    /// Capture chain warmed up; ready tone + guard interval elapsed.
    CaptureWarmedUp,
}

/// Side effects the state machine requests the caller to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceAction {
    /// No action.
    None,
    /// Send a TALK_STATE packet (action 1=start, 0=end).
    SendTalkState { action: u8 },
    /// start_capture() — warms up the capture chain; early frames are discarded.
    StartCapture,
    /// stop_capture().
    StopCapture,
    /// pa_enable(true|false).
    PaEnable { on: bool },
    /// Play ready tone (then caller emits CaptureWarmedUp after guard).
    PlayReadyTone,
    /// Play busy tone (then caller emits BusyToneDone).
    PlayBusyTone,
    /// Turn the screen on (skipped when screen_was_off==true per D8).
    ScreenOn,
    /// Arm capture — subsequent capture frames are encoded + sent.
    ArmCapture,
    /// Disarm capture — discard capture frames until armed again.
    DisarmCapture,
    /// Start playing back the VoiceChanger preview buffer (change
    /// intercom-voice-changer-preview). The controller reads the processed
    /// buffer from `IntercomApp::preview_buffer` and submits it to the audio
    /// service. Unit variant (no payload) because `VoiceAction` is `Copy`.
    StartPreviewPlayback,
    /// Stop the VoiceChanger preview playback.
    StopPreviewPlayback,
}

/// Outcome of handling an event: new state + actions to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceOutcome {
    pub new_state: VoiceState,
    pub actions: Vec<VoiceAction>,
}

/// Pure-logic PTT state machine.
pub struct VoicePttMachine {
    state: VoiceState,
    mode: IntercomMode,
    capture_armed: bool,
    /// Set true when in clear-mode PTT arbitration window. Distinguishes
    /// "Idle because never pressed" from "Idle because waiting for arb".
    in_arbitration: bool,
    /// Tracks whether we ever entered Talking during this PTT cycle (for
    /// release-time TALK_STATE(action=0) — only emit if we actually talked).
    talked_this_cycle: bool,
    /// Last sender we saw talking (for Listening→Idle transitions).
    last_talking_sender: u8,
}

impl fmt::Debug for VoicePttMachine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoicePttMachine")
            .field("state", &self.state)
            .field("mode", &self.mode)
            .field("capture_armed", &self.capture_armed)
            .finish_non_exhaustive()
    }
}

impl VoicePttMachine {
    pub fn new(mode: IntercomMode) -> Self {
        Self {
            state: VoiceState::Idle,
            mode,
            capture_armed: false,
            in_arbitration: false,
            talked_this_cycle: false,
            last_talking_sender: 0,
        }
    }

    pub fn state(&self) -> VoiceState {
        self.state
    }

    pub fn set_mode(&mut self, mode: IntercomMode) {
        self.mode = mode;
    }

    pub fn capture_armed(&self) -> bool {
        self.capture_armed
    }

    /// Reset to Idle (e.g. on group leave).
    pub fn reset(&mut self) {
        self.state = VoiceState::Idle;
        self.capture_armed = false;
        self.in_arbitration = false;
        self.talked_this_cycle = false;
    }

    pub fn handle(&mut self, event: VoiceEvent) -> VoiceOutcome {
        use VoiceAction::*;
        use VoiceEvent::*;
        use VoiceState::*;

        let mut actions: Vec<VoiceAction> = Vec::new();
        let prev = self.state;

        match (prev, event) {
            // ---- PTT press from Idle ----
            // D8: screen_was_off=true means PTT bypass — don't wake screen,
            // just proceed with PTT. screen_was_off=false is normal PTT.
            // Either way, the PTT flow is the same; the ScreenOn action is
            // NOT emitted (caller handles wake separately if needed).
            (Idle, PttPress { screen_was_off: _ }) => {
                match self.mode {
                    IntercomMode::Clear => {
                        // clear-mode: send TALK_STATE(action=1), enter arbitration.
                        actions.push(SendTalkState { action: 1 });
                        self.in_arbitration = true;
                        // Stay in Idle; arbitration outcome drives the next transition.
                    }
                    IntercomMode::Free => {
                        // free-mode: no arbitration, direct to Talking.
                        actions.push(SendTalkState { action: 1 });
                        actions.push(StartCapture);
                        actions.push(PaEnable { on: true });
                        actions.push(PlayReadyTone);
                        self.state = Talking;
                        self.talked_this_cycle = true;
                        // capture_armed set true after CaptureWarmedUp event.
                    }
                }
            }

            // ---- Arbitration outcome (clear-mode only, gated by in_arbitration flag) ----
            (Idle, ArbitrationTimeout) if self.in_arbitration => {
                // No conflict in 50ms → enter Talking.
                self.in_arbitration = false;
                actions.push(StartCapture);
                actions.push(PaEnable { on: true });
                actions.push(PlayReadyTone);
                self.state = Talking;
                self.talked_this_cycle = true;
            }
            (Idle, TalkStateReceived { action: 1, from_sender }) if self.in_arbitration => {
                // Conflict during arbitration → ChannelBusy.
                self.in_arbitration = false;
                self.last_talking_sender = from_sender;
                actions.push(PlayBusyTone);
                self.state = ChannelBusy;
                self.talked_this_cycle = false;
            }

            // ---- Capture warmed up (ready tone + guard done) ----
            (Talking, CaptureWarmedUp) => {
                actions.push(ArmCapture);
                self.capture_armed = true;
            }

            // ---- PTT release ----
            (Talking, PttRelease) => {
                actions.push(DisarmCapture);
                self.capture_armed = false;
                actions.push(StopCapture);
                actions.push(PaEnable { on: false });
                if self.talked_this_cycle {
                    actions.push(SendTalkState { action: 0 });
                }
                self.state = Idle;
                self.talked_this_cycle = false;
            }
            (Idle, PttRelease) if self.in_arbitration => {
                // Released during arbitration (clear-mode) — cancel.
                self.in_arbitration = false;
                if self.talked_this_cycle {
                    actions.push(SendTalkState { action: 0 });
                }
                self.state = Idle;
            }
            (ChannelBusy, PttRelease) => {
                // User released during busy tone — just go Idle.
                self.state = Idle;
            }
            (Listening, PttRelease) => {
                // Listening is a receive-only state; release is a no-op.
            }

            // ---- Remote TALK_STATE handling ----
            (Idle, TalkStateReceived { action: 1, from_sender }) => {
                self.last_talking_sender = from_sender;
                self.state = Listening;
            }
            (Listening, TalkStateReceived { action: 0, .. }) => {
                self.state = Idle;
            }
            (Listening, TalkStateReceived { action: 1, from_sender }) => {
                // Another peer starts talking while we're already Listening.
                self.last_talking_sender = from_sender;
            }
            (Talking, TalkStateReceived { action: 1, .. }) => {
                // D9: near-simultaneous clear-mode collision — known acceptable
                // degradation. Stay in Talking; recover on next cycle.
            }
            (ChannelBusy, TalkStateReceived { action: 0, .. }) => {
                // Remote released — but D6: don't auto-recover, wait for BusyToneDone.
            }

            // ---- Busy tone done ----
            (ChannelBusy, BusyToneDone) => {
                self.state = Idle;
            }

            // ---- Default: ignore unmatched transitions ----
            _ => {
                actions.push(None);
            }
        }

        VoiceOutcome {
            new_state: self.state,
            actions,
        }
    }
}

// ---- Trait shim for callers expecting a service-like object ---------------

pub trait VoicePttService: Send + Sync + fmt::Debug {
    fn on_boot_press(&self, screen_was_off: bool);
    fn on_boot_release(&self);
    fn current_state(&self) -> VoiceState;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() -> VoicePttMachine {
        VoicePttMachine::new(IntercomMode::Clear)
    }
    fn free() -> VoicePttMachine {
        VoicePttMachine::new(IntercomMode::Free)
    }

    #[test]
    fn clear_mode_press_arbitrates_then_talks() {
        let mut m = clear();
        let o = m.handle(VoiceEvent::PttPress { screen_was_off: false });
        // Should send TALK_STATE(action=1) and stay Idle (waiting for arbitration).
        assert!(o.actions.contains(&VoiceAction::SendTalkState { action: 1 }));
        assert_eq!(o.new_state, VoiceState::Idle);

        // Arbitration timeout → enter Talking.
        let o2 = m.handle(VoiceEvent::ArbitrationTimeout);
        assert_eq!(o2.new_state, VoiceState::Talking);
        assert!(o2.actions.contains(&VoiceAction::StartCapture));
        assert!(o2.actions.contains(&VoiceAction::PaEnable { on: true }));

        // Warm-up → arm capture.
        let o3 = m.handle(VoiceEvent::CaptureWarmedUp);
        assert!(o3.actions.contains(&VoiceAction::ArmCapture));
        assert!(m.capture_armed());

        // Release → disarm, stop, pa off, send action=0, back to Idle.
        let o4 = m.handle(VoiceEvent::PttRelease);
        assert_eq!(o4.new_state, VoiceState::Idle);
        assert!(o4.actions.contains(&VoiceAction::StopCapture));
        assert!(o4.actions.contains(&VoiceAction::PaEnable { on: false }));
        assert!(o4.actions.contains(&VoiceAction::SendTalkState { action: 0 }));
        assert!(!m.capture_armed());
    }

    #[test]
    fn clear_mode_conflict_goes_to_busy() {
        let mut m = clear();
        m.handle(VoiceEvent::PttPress { screen_was_off: false });
        // Peer starts talking during arbitration.
        let o = m.handle(VoiceEvent::TalkStateReceived { action: 1, from_sender: 2 });
        assert_eq!(o.new_state, VoiceState::ChannelBusy);
        assert!(o.actions.contains(&VoiceAction::PlayBusyTone));

        // Busy tone done → Idle.
        let o2 = m.handle(VoiceEvent::BusyToneDone);
        assert_eq!(o2.new_state, VoiceState::Idle);
    }

    #[test]
    fn free_mode_skips_arbitration() {
        let mut m = free();
        let o = m.handle(VoiceEvent::PttPress { screen_was_off: false });
        assert_eq!(o.new_state, VoiceState::Talking);
        // No ArbitrationTimeout event expected by caller in free mode.
        assert!(!o.actions.contains(&VoiceAction::PlayBusyTone));
    }

    #[test]
    fn remote_talk_state_transitions_listening() {
        let mut m = clear();
        let o = m.handle(VoiceEvent::TalkStateReceived { action: 1, from_sender: 3 });
        assert_eq!(o.new_state, VoiceState::Listening);
        let o2 = m.handle(VoiceEvent::TalkStateReceived { action: 0, from_sender: 3 });
        assert_eq!(o2.new_state, VoiceState::Idle);
    }

    #[test]
    fn release_during_arbitration_cancels() {
        let mut m = clear();
        m.handle(VoiceEvent::PttPress { screen_was_off: false });
        // Release before arbitration resolves.
        let o = m.handle(VoiceEvent::PttRelease);
        assert_eq!(o.new_state, VoiceState::Idle);
        // Should NOT send action=0 (we never entered Talking).
        assert!(!o.actions.contains(&VoiceAction::SendTalkState { action: 0 }));
    }

    #[test]
    fn screen_off_press_does_not_emit_screen_on() {
        let mut m = clear();
        let o = m.handle(VoiceEvent::PttPress { screen_was_off: true });
        // No ScreenOn action expected (D8).
        assert!(!o.actions.contains(&VoiceAction::ScreenOn));
    }

    #[test]
    fn reset_clears_state() {
        let mut m = free();
        m.handle(VoiceEvent::PttPress { screen_was_off: false });
        m.reset();
        assert_eq!(m.state(), VoiceState::Idle);
        assert!(!m.capture_armed());
    }
}
