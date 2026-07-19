//! Intercom app UI (change 13). Spec: §3.7, §15, §13.2.
//!
//! Pure-logic intercom app state machine. Maps PTT touch/button events to
//! IntercomService calls, tracks UI state (Idle / Listening / PttArming /
//! PttActive), displays peer cards with signal bars + voice-active indicator.
//! Slint rendering lands in change 17; this module hosts the testable state.

#![allow(dead_code)]

use std::fmt;

use crate::intercom::state::{IntercomEvent, IntercomMode, VoiceState, VoiceEffect};
use crate::intercom::voice::{VoiceAction, VoiceEvent, VoicePttMachine};
use crate::intercom::voice_changer::apply_effect;
use crate::intercom::pairing::{DiscoveredHost, HostInfo};
use crate::services::input::InputEvent;
use crate::apps::view::intercom_view::IntercomPage;
use crate::apps::{App, AppContext, HitTarget, RenderCtx};
use crate::services::display_buf::Rgb565Buf;

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

/// Pairing entry actions produced by the ungrouped UI (change
/// intercom-ungrouped-ui) and routed by the controller to
/// `IntercomService::set_state`. change: wire-network-runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingAction {
    /// User tapped "create group" → become host.
    StartHost,
    /// User tapped "search groups" → scan for host beacons.
    SearchHosts,
    /// User selected a discovered host (by MAC) to join.
    Join([u8; 6]),
    /// User cancelled pairing (back button while searching/hosting).
    Cancel,
}

/// VoiceChanger preview sub-state machine (change
/// intercom-voice-changer-preview). Tapping an effect records 3s of audio,
/// applies the effect, then plays it back so the user hears the result
/// before committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceChangerSubState {
    /// Showing the 3 effect buttons, waiting for a tap.
    Idle,
    /// Capturing 3s of audio into the preview buffer, counting down.
    Recording { remain_ms: u32, target: VoiceEffect },
    /// Playing back the effect-applied buffer, counting down.
    Previewing { remain_ms: u32, effect: VoiceEffect },
}

/// Preview record/playback durations (ms) and buffer size. 8kHz mono × 3s =
/// 24000 samples ≈ 48KB (SRAM budget, design §SRAM).
pub const VC_RECORD_MS: u32 = 3000;
pub const VC_PREVIEW_MS: u32 = 3000;
pub const VC_PREVIEW_SAMPLES: usize = 24000;

/// Outcome of an intercom-app event dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntercomAppOutcome {
    pub new_state: IntercomUiState,
    pub voice_actions: Vec<VoiceAction>,
    pub refresh_peer_cards: bool,
    /// Pairing entry action to route to `IntercomService` (None for PTT/voice
    /// dispatches). change: wire-network-runtime.
    pub pairing_action: Option<PairingAction>,
}

pub struct IntercomApp {
    ui_state: IntercomUiState,
    ptt: VoicePttMachine,
    mode: IntercomMode,
    peers: Vec<PeerCard>,
    page: IntercomPage,
    /// True while the channel is busy (a peer won arbitration). Set on
    /// `IntercomEvent::ChannelBusy`, auto-cleared by the controller after 2s.
    /// change: wire-ptt-end-to-end.
    channel_busy: bool,
    /// Discovered hosts for the ungrouped Search page (change
    /// intercom-ungrouped-ui). Populated by the controller via
    /// `set_discovered_hosts` from the pairing machine's host list.
    discovered_hosts: Vec<DiscoveredHost>,
    /// Index into `discovered_hosts` selected on the Search page.
    selected_host: Option<usize>,
    /// Last join-rejection reason, shown as a toast on the home page.
    join_error: Option<String>,
    /// Count of peers that have joined while hosting (Creating page).
    creating_peer_count: u32,
    /// True while the GroupInfo leave-confirmation modal is showing (change
    /// intercom-group-info-leave). The modal masks all other GroupInfo hits.
    confirming_leave: bool,
    /// VoiceChanger preview sub-state (change intercom-voice-changer-preview).
    vc_state: VoiceChangerSubState,
    /// Preview PCM buffer: raw capture during Recording, effect-applied output
    /// during Previewing. Pre-allocated once to avoid mid-record allocation.
    preview_buffer: Vec<i16>,
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
            page: IntercomPage::UngroupedHome,
            channel_busy: false,
            discovered_hosts: Vec::new(),
            selected_host: None,
            join_error: None,
            creating_peer_count: 0,
            confirming_leave: false,
            vc_state: VoiceChangerSubState::Idle,
            preview_buffer: Vec::with_capacity(VC_PREVIEW_SAMPLES),
        }
    }

    pub fn ui_state(&self) -> IntercomUiState {
        self.ui_state
    }

    /// Whether the channel is currently busy (a peer is talking and we lost
    /// arbitration). Drives the PTT-degrade log/visual. change: wire-ptt-end-to-end.
    pub fn channel_busy(&self) -> bool {
        self.channel_busy
    }

    /// Clear the channel-busy flag (controller calls this ~2s after the
    /// busy event). Also recovers the UI state to Idle if still ChannelBusy.
    pub fn clear_channel_busy(&mut self) {
        self.channel_busy = false;
        if self.ui_state == IntercomUiState::ChannelBusy {
            self.ui_state = IntercomUiState::Idle;
        }
    }

    pub fn page(&self) -> IntercomPage {
        self.page
    }

    pub fn set_page(&mut self, page: IntercomPage) {
        self.page = page;
    }

    // ---- Ungrouped pairing-entry accessors (change intercom-ungrouped-ui) --

    pub fn discovered_hosts(&self) -> &[DiscoveredHost] {
        &self.discovered_hosts
    }

    /// Replace the discovered-host list (controller pushes from the pairing
    /// machine's `host_list`). Clears a now-out-of-range selection.
    pub fn set_discovered_hosts(&mut self, hosts: Vec<DiscoveredHost>) {
        if let Some(idx) = self.selected_host {
            if idx >= hosts.len() {
                self.selected_host = None;
            }
        }
        self.discovered_hosts = hosts;
    }

    pub fn selected_host(&self) -> Option<usize> {
        self.selected_host
    }

    pub fn join_error(&self) -> Option<&str> {
        self.join_error.as_deref()
    }

    pub fn clear_join_error(&mut self) {
        self.join_error = None;
    }

    pub fn creating_peer_count(&self) -> u32 {
        self.creating_peer_count
    }

    /// Home → tap "Create Group": switch to CreatingHost + emit StartHost.
    pub fn tap_create_host(&mut self) -> PairingAction {
        self.join_error = None;
        self.creating_peer_count = 0;
        self.page = IntercomPage::CreatingHost;
        PairingAction::StartHost
    }

    /// Home → tap "Search Groups": switch to SearchingHosts + emit SearchHosts.
    pub fn tap_search_hosts(&mut self) -> PairingAction {
        self.join_error = None;
        self.discovered_hosts.clear();
        self.selected_host = None;
        self.page = IntercomPage::SearchingHosts;
        PairingAction::SearchHosts
    }

    /// Search → tap "Refresh": clear + rescan.
    pub fn tap_refresh(&mut self) -> PairingAction {
        self.discovered_hosts.clear();
        self.selected_host = None;
        PairingAction::SearchHosts
    }

    /// Search → tap a list row: select it (bounds-checked).
    pub fn tap_host_item(&mut self, idx: usize) {
        if idx < self.discovered_hosts.len() {
            self.selected_host = Some(idx);
        }
    }

    /// Search → tap "Join": if a host is selected, switch to JoiningHost and
    /// emit `Join(mac)` with the selected host's MAC. Returns None otherwise.
    pub fn tap_join(&mut self) -> Option<PairingAction> {
        let idx = self.selected_host?;
        let host = self.discovered_hosts.get(idx)?;
        let mac = host.host_mac;
        self.page = IntercomPage::JoiningHost;
        Some(PairingAction::Join(mac))
    }

    /// Creating/Searching/Joining → tap "Back"/"Cancel": return to home + Cancel.
    pub fn tap_pairing_back(&mut self) -> PairingAction {
        self.page = IntercomPage::UngroupedHome;
        self.selected_host = None;
        PairingAction::Cancel
    }

    // ---- GroupInfo leave-group flow (change intercom-group-info-leave) ------

    pub fn confirming_leave(&self) -> bool {
        self.confirming_leave
    }

    /// GroupInfo → tap "Leave Group": show the confirmation modal.
    pub fn tap_leave_group(&mut self) {
        self.confirming_leave = true;
    }

    /// Modal → tap "Confirm": hide the modal and signal the controller to call
    /// `IntercomService::leave_group()`. Returns true when a leave was confirmed.
    pub fn tap_confirm_leave(&mut self) -> bool {
        self.confirming_leave = false;
        true
    }

    /// Modal → tap "Cancel": dismiss the modal, stay in the group.
    pub fn tap_cancel_leave(&mut self) {
        self.confirming_leave = false;
    }

    // ---- VoiceChanger preview (change intercom-voice-changer-preview) -------

    pub fn vc_state(&self) -> VoiceChangerSubState {
        self.vc_state
    }

    /// Effect-applied preview buffer (valid while Previewing). The controller
    /// submits this to the audio service on `StartPreviewPlayback`.
    pub fn preview_buffer(&self) -> &[i16] {
        &self.preview_buffer
    }

    /// True when a PTT call is active (any non-Idle UI state). The VoiceChanger
    /// page disables preview during a call (design §Risks).
    pub fn in_call(&self) -> bool {
        self.ui_state != IntercomUiState::Idle
    }

    /// Append captured PCM into the preview buffer while Recording (controller
    /// hook from the capture path). Caps at `VC_PREVIEW_SAMPLES`.
    ///
    /// NOTE: the raw-PCM capture tap from the audio thread to the main thread
    /// is not yet exposed (change wire-audio-pipeline encodes in-thread), so
    /// this is currently unused in the wired path — the preview plays silence
    /// until the capture tap lands (task 6.1, verified on-device).
    pub fn push_preview_frame(&mut self, pcm: &[i16]) {
        if !matches!(self.vc_state, VoiceChangerSubState::Recording { .. }) {
            return;
        }
        let room = VC_PREVIEW_SAMPLES.saturating_sub(self.preview_buffer.len());
        let take = room.min(pcm.len());
        self.preview_buffer.extend_from_slice(&pcm[..take]);
    }

    /// Tap one of the 3 effect buttons. Blocked during a call. From Idle starts
    /// a 3s recording; from Previewing restarts recording with the new effect;
    /// ignored while already Recording.
    pub fn tap_voice_effect(&mut self, effect: VoiceEffect) -> Vec<VoiceAction> {
        if self.in_call() {
            return Vec::new();
        }
        match self.vc_state {
            VoiceChangerSubState::Idle => {
                self.preview_buffer.clear();
                self.vc_state = VoiceChangerSubState::Recording {
                    remain_ms: VC_RECORD_MS,
                    target: effect,
                };
                vec![VoiceAction::StartCapture]
            }
            VoiceChangerSubState::Previewing { .. } => {
                self.preview_buffer.clear();
                self.vc_state = VoiceChangerSubState::Recording {
                    remain_ms: VC_RECORD_MS,
                    target: effect,
                };
                vec![VoiceAction::StopPreviewPlayback, VoiceAction::StartCapture]
            }
            VoiceChangerSubState::Recording { .. } => Vec::new(),
        }
    }

    /// Advance the preview sub-state by `dt_ms` (called each main-loop tick).
    /// Recording→Previewing applies the effect; Previewing→Idle stops playback.
    pub fn tick_voice_changer(&mut self, dt_ms: u32) -> Vec<VoiceAction> {
        match self.vc_state {
            VoiceChangerSubState::Recording { remain_ms, target } => {
                if remain_ms <= dt_ms {
                    // Record done: apply the effect in place, then preview.
                    let processed = apply_effect(&self.preview_buffer, target);
                    self.preview_buffer = processed;
                    self.vc_state = VoiceChangerSubState::Previewing {
                        remain_ms: VC_PREVIEW_MS,
                        effect: target,
                    };
                    vec![VoiceAction::StopCapture, VoiceAction::StartPreviewPlayback]
                } else {
                    self.vc_state = VoiceChangerSubState::Recording {
                        remain_ms: remain_ms - dt_ms,
                        target,
                    };
                    Vec::new()
                }
            }
            VoiceChangerSubState::Previewing { remain_ms, effect } => {
                if remain_ms <= dt_ms {
                    self.vc_state = VoiceChangerSubState::Idle;
                    vec![VoiceAction::StopPreviewPlayback]
                } else {
                    self.vc_state = VoiceChangerSubState::Previewing {
                        remain_ms: remain_ms - dt_ms,
                        effect,
                    };
                    Vec::new()
                }
            }
            VoiceChangerSubState::Idle => Vec::new(),
        }
    }

    /// Cancel an in-progress record/preview, returning to Idle.
    pub fn cancel_voice_preview(&mut self) -> Vec<VoiceAction> {
        match self.vc_state {
            VoiceChangerSubState::Recording { .. } => {
                self.vc_state = VoiceChangerSubState::Idle;
                vec![VoiceAction::StopCapture]
            }
            VoiceChangerSubState::Previewing { .. } => {
                self.vc_state = VoiceChangerSubState::Idle;
                vec![VoiceAction::StopPreviewPlayback]
            }
            VoiceChangerSubState::Idle => Vec::new(),
        }
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
    /// Touch Down → PttPress (task 9.1: touch-PTT entry).
    /// Touch Up → PttRelease (task 9.2).
    /// `screen_was_off` is always false for touch PTT — the ScreenPolicy
    /// consumes the first wake touch, so by the time we get here the screen
    /// is on.
    fn input_to_voice_event(ev: &InputEvent) -> Option<VoiceEvent> {
        match ev {
            InputEvent::BootPress { screen_was_off } => Some(VoiceEvent::PttPress {
                screen_was_off: *screen_was_off,
            }),
            InputEvent::BootRelease => Some(VoiceEvent::PttRelease),
            InputEvent::Touch(crate::hal::touch::TouchEvent::Down { .. }) => {
                Some(VoiceEvent::PttPress { screen_was_off: false })
            }
            InputEvent::Touch(crate::hal::touch::TouchEvent::Up { .. }) => {
                Some(VoiceEvent::PttRelease)
            }
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
            pairing_action: None,
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
            pairing_action: None,
        }
    }

    /// Consume an `IntercomEvent` delivered from the network/coordinator layer
    /// (drained from the UiEvent queue by the main loop). Updates peer-card
    /// roster + UI state. Returns voice actions to execute (empty here — voice
    /// packet handling is wired in wire-ptt-end-to-end / wire-audio-pipeline).
    /// change: wire-network-runtime.
    pub fn on_intercom_event(&mut self, ev: IntercomEvent) -> Vec<VoiceAction> {
        match ev {
            IntercomEvent::TalkState { sender_id, talking } => {
                self.on_peer_voice_state(sender_id, talking);
            }
            IntercomEvent::VoiceActive(sender_id) => {
                self.on_peer_voice_state(sender_id, true);
            }
            IntercomEvent::PeerOnline(sender_id, rssi_bars) => {
                if let Some(p) = self.peers.iter_mut().find(|p| p.sender_id == sender_id) {
                    p.online = true;
                    p.rssi_bars = rssi_bars;
                }
                // While hosting, a peer coming online bumps the join count
                // shown on the CreatingHost page (task 3.3, adapted: real
                // schema has no PeerJoined event).
                if self.page == IntercomPage::CreatingHost {
                    self.creating_peer_count = self.creating_peer_count.saturating_add(1);
                }
            }
            IntercomEvent::PeerOffline(sender_id) => {
                if let Some(p) = self.peers.iter_mut().find(|p| p.sender_id == sender_id) {
                    p.online = false;
                }
            }
            IntercomEvent::ChannelBusy => {
                self.ui_state = IntercomUiState::ChannelBusy;
                self.channel_busy = true;
            }
            IntercomEvent::PttReady => {
                if self.ui_state == IntercomUiState::ChannelBusy {
                    self.ui_state = IntercomUiState::Idle;
                }
            }
            IntercomEvent::GroupFormed => {
                self.page = IntercomPage::Main;
                self.ui_state = IntercomUiState::Idle;
                self.join_error = None;
                self.selected_host = None;
                self.discovered_hosts.clear();
            }
            IntercomEvent::JoinAccepted => {
                // Host approved our join → enter the grouped Main page.
                self.page = IntercomPage::Main;
                self.ui_state = IntercomUiState::Idle;
                self.join_error = None;
            }
            IntercomEvent::JoinRejected(reason) => {
                // Bounce back to the home entry with a toast (tasks 5.4 / 7.1).
                self.page = IntercomPage::UngroupedHome;
                self.selected_host = None;
                self.join_error = Some(reason);
            }
            IntercomEvent::LeftGroup => {
                self.peers.clear();
                self.page = IntercomPage::UngroupedHome;
                self.ui_state = IntercomUiState::Idle;
                self.confirming_leave = false;
            }
            // Roster/pairing-progress events: the render snapshot rebuilds the
            // peer cards / host list, so no direct state mutation is needed here.
            IntercomEvent::PeerListChanged
            | IntercomEvent::StateChanged(_)
            | IntercomEvent::PairFailed(_)
            | IntercomEvent::HostDiscovered(_) => {}
        }
        Vec::new()
    }
}

impl App for IntercomApp {
    fn id(&self) -> &str { "intercom" }
    fn title(&self) -> &str { "Intercom" }
    fn on_enter(&mut self, _ctx: &AppContext) {}
    fn on_exit(&mut self, _ctx: &AppContext) {}
    fn on_event(&mut self, _ev: &InputEvent, _ctx: &AppContext) {
        // PTT input routing handled by dispatch() (called directly by
        // controller in dispatch_touch/dispatch_button).
    }
    fn on_tick(&mut self, _ctx: &AppContext) {}
    fn render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx) {
        crate::apps::view::intercom_view::draw_intercom(fb, ctx, self, self.page);
    }
    fn hit_test(&self, x: i32, y: i32, _ctx: &RenderCtx) -> Option<HitTarget> {
        crate::apps::view::intercom_view::hit_test(x, y, self.page, self.confirming_leave, self.vc_state)
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

    fn mk_host(mac_last: u8) -> DiscoveredHost {
        DiscoveredHost {
            host_mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, mac_last],
            host_pub_key: [1u8; 32],
            mode: IntercomMode::Clear,
            cur_members: 1,
            max_members: 4,
            joinable: true,
            rssi_4bar: 3,
            last_seen_ms: 0,
        }
    }

    #[test]
    fn ungrouped_defaults_to_home() {
        let a = mk();
        assert_eq!(a.page(), IntercomPage::UngroupedHome);
    }

    #[test]
    fn create_host_flow_to_grouped_main() {
        let mut a = mk();
        assert_eq!(a.tap_create_host(), PairingAction::StartHost);
        assert_eq!(a.page(), IntercomPage::CreatingHost);
        // A peer coming online while hosting bumps the join count.
        a.on_intercom_event(IntercomEvent::PeerOnline(2, 3));
        assert_eq!(a.creating_peer_count(), 1);
        // Group forms → Main.
        a.on_intercom_event(IntercomEvent::GroupFormed);
        assert_eq!(a.page(), IntercomPage::Main);
    }

    #[test]
    fn search_select_join_flow() {
        let mut a = mk();
        assert_eq!(a.tap_search_hosts(), PairingAction::SearchHosts);
        assert_eq!(a.page(), IntercomPage::SearchingHosts);
        a.set_discovered_hosts(vec![mk_host(0x01), mk_host(0x02)]);
        // Join without a selection is a no-op.
        assert_eq!(a.tap_join(), None);
        a.tap_host_item(1);
        assert_eq!(a.selected_host(), Some(1));
        assert_eq!(a.tap_join(), Some(PairingAction::Join([0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02])));
        assert_eq!(a.page(), IntercomPage::JoiningHost);
    }

    #[test]
    fn join_rejected_shows_error_and_returns_home() {
        let mut a = mk();
        a.tap_search_hosts();
        a.set_discovered_hosts(vec![mk_host(0x01)]);
        a.tap_host_item(0);
        a.tap_join();
        a.on_intercom_event(IntercomEvent::JoinRejected("group full".into()));
        assert_eq!(a.page(), IntercomPage::UngroupedHome);
        assert_eq!(a.join_error(), Some("group full"));
        assert_eq!(a.selected_host(), None);
    }

    #[test]
    fn join_accepted_enters_main() {
        let mut a = mk();
        a.tap_search_hosts();
        a.set_discovered_hosts(vec![mk_host(0x01)]);
        a.tap_host_item(0);
        a.tap_join();
        a.on_intercom_event(IntercomEvent::JoinAccepted);
        assert_eq!(a.page(), IntercomPage::Main);
    }

    // ---- change intercom-group-info-leave -------------------------------

    #[test]
    fn leave_group_confirm_flow() {
        let mut a = mk_with_peers();
        a.on_intercom_event(IntercomEvent::GroupFormed); // enter Main
        assert!(!a.confirming_leave());
        // Tap Leave → modal opens.
        a.tap_leave_group();
        assert!(a.confirming_leave());
        // Confirm → returns true (controller then calls leave_group) and
        // dismisses the modal.
        assert!(a.tap_confirm_leave());
        assert!(!a.confirming_leave());
        // LeftGroup event flips back to the ungrouped home + clears peers.
        a.on_intercom_event(IntercomEvent::LeftGroup);
        assert_eq!(a.page(), IntercomPage::UngroupedHome);
        assert!(a.peers().is_empty());
        assert!(!a.confirming_leave());
    }

    #[test]
    fn leave_group_cancel_keeps_group() {
        let mut a = mk_with_peers();
        a.on_intercom_event(IntercomEvent::GroupFormed);
        a.tap_leave_group();
        assert!(a.confirming_leave());
        // Cancel → modal dismissed, still in Main with peers intact.
        a.tap_cancel_leave();
        assert!(!a.confirming_leave());
        assert_eq!(a.page(), IntercomPage::Main);
        assert_eq!(a.peers().len(), 2);
    }

    // ---- change intercom-voice-changer-preview --------------------------

    #[test]
    fn vc_full_cycle_idle_recording_previewing_idle() {
        let mut a = mk();
        assert_eq!(a.vc_state(), VoiceChangerSubState::Idle);
        // Idle → tap PitchUp → Recording + StartCapture.
        let acts = a.tap_voice_effect(VoiceEffect::PitchUp);
        assert!(acts.contains(&VoiceAction::StartCapture));
        assert!(matches!(a.vc_state(), VoiceChangerSubState::Recording { .. }));
        // Record 3000ms (60 × 50ms) → Previewing + StartPreviewPlayback.
        let mut to_preview = false;
        for _ in 0..60 {
            if a.tick_voice_changer(50).contains(&VoiceAction::StartPreviewPlayback) {
                to_preview = true;
            }
        }
        assert!(to_preview);
        assert!(matches!(a.vc_state(), VoiceChangerSubState::Previewing { .. }));
        // Preview 3000ms → Idle + StopPreviewPlayback.
        let mut to_idle = false;
        for _ in 0..60 {
            if a.tick_voice_changer(50).contains(&VoiceAction::StopPreviewPlayback) {
                to_idle = true;
            }
        }
        assert!(to_idle);
        assert_eq!(a.vc_state(), VoiceChangerSubState::Idle);
    }

    #[test]
    fn vc_recording_ignores_further_taps() {
        let mut a = mk();
        a.tap_voice_effect(VoiceEffect::PitchUp);
        // Tapping another effect while Recording is a no-op.
        let acts = a.tap_voice_effect(VoiceEffect::PitchDown);
        assert!(acts.is_empty());
        assert!(matches!(
            a.vc_state(),
            VoiceChangerSubState::Recording { target: VoiceEffect::PitchUp, .. }
        ));
    }

    #[test]
    fn vc_cancel_returns_to_idle() {
        let mut a = mk();
        a.tap_voice_effect(VoiceEffect::PitchUp);
        let acts = a.cancel_voice_preview();
        assert!(acts.contains(&VoiceAction::StopCapture));
        assert_eq!(a.vc_state(), VoiceChangerSubState::Idle);
    }

    #[test]
    fn vc_blocked_during_call() {
        let mut a = IntercomApp::new(IntercomMode::Free);
        // PTT press → PttActive (in a call).
        a.dispatch(&InputEvent::BootPress { screen_was_off: false });
        assert!(a.in_call());
        let acts = a.tap_voice_effect(VoiceEffect::PitchUp);
        assert!(acts.is_empty());
        assert_eq!(a.vc_state(), VoiceChangerSubState::Idle);
    }
}
