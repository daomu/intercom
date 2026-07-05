//! Restore + heartbeat (change 12). Spec: §3.5, §5.2 cold-boot restore,
//! §13.1 heartbeat periods, §13.2 RSSI/offline rules.
//!
//! Pure-logic components:
//! - `HeartbeatTracker`: per-peer last_seen + RSSI EWMA + 4-bar hysteresis
//! - `heartbeat_period(state, screen_off) -> Duration`: state-aware period map
//! - `rssi_to_bars(...)`: 4-bar mapping with ±3 dB hysteresis
//!
//! `restore_from_nvs()` orchestration (read NVS → clear on schema fail →
//! init network at g.channel → re-derive LMKs → enter Grouped(Idle)) is
//! expressed as a `RestoreOutcome` so the caller (IntercomService on Task B)
//! can execute the side-effecting steps.

#![allow(dead_code)]

use std::fmt;
use std::time::Duration;

use crate::intercom::state::{IntercomState, VoiceState};
use crate::services::storage::{GroupInfo, PeerEntry};

// ---- Constants (PRD §13.1, §13.2, §15.1) ---------------------------------

pub const HB_PERIOD_IDLE: Duration = Duration::from_secs(5);
pub const HB_PERIOD_PAIRING: Duration = Duration::from_secs(1);
pub const HB_PERIOD_VOICE: Duration = Duration::from_secs(10);
pub const HB_PERIOD_SCREEN_OFF: Duration = Duration::from_secs(10);
pub const OFFLINE_TIMEOUT: Duration = Duration::from_secs(15);

/// EWMA alpha for RSSI smoothing (D7).
pub const RSSI_EWMA_ALPHA_NUM: i32 = 3;
pub const RSSI_EWMA_ALPHA_DEN: i32 = 10;

/// Hysteresis band around 4-bar thresholds (±3 dB, D7).
pub const RSSI_HYSTERESIS_DB: i32 = 3;

// ---- Heartbeat period function (D4) --------------------------------------

/// Returns the heartbeat period for the given state. `screen_off` overrides
/// to 10s when the screen is off regardless of VoiceState (D4).
pub fn heartbeat_period(state: &IntercomState, screen_off: bool) -> Duration {
    if screen_off {
        return HB_PERIOD_SCREEN_OFF;
    }
    match state {
        IntercomState::Grouped(VoiceState::Talking) | IntercomState::Grouped(VoiceState::Listening) => {
            HB_PERIOD_VOICE
        }
        IntercomState::Hosting(_) | IntercomState::Joining(_) => HB_PERIOD_PAIRING,
        IntercomState::Grouped(_) | IntercomState::Idle => HB_PERIOD_IDLE,
    }
}

// ---- RSSI mapping (D7) ----------------------------------------------------

/// Map a raw RSSI (dBm, negative) to a 4-bar level without hysteresis.
pub fn rssi_to_bars_raw(rssi: i32) -> u8 {
    if rssi >= -55 { 4 }
    else if rssi >= -65 { 3 }
    else if rssi >= -75 { 2 }
    else if rssi >= -85 { 1 }
    else { 0 }
}

/// Map a smoothed RSSI to 4-bar with ±3 dB hysteresis around the current
/// level's thresholds (D7).
pub fn rssi_to_bars(smoothed: i32, current_bars: u8) -> u8 {
    // Thresholds: bar N requires rssi >= thr[N].
    let thr = [-55, -65, -75, -85];
    let cur = current_bars.min(4) as usize;
    // Move up only if smoothed exceeds thr[cur-1] + hysteresis.
    // Move down only if smoothed drops below thr[cur] - hysteresis (when cur>0).
    if cur < 4 && smoothed >= thr[cur.saturating_sub(1)] + RSSI_HYSTERESIS_DB {
        // Promote: at boundary, thr indexing: bar (cur+1) requires >= thr[cur]
        // We want to move up when smoothed is solidly above the next-up threshold.
        return (cur + 1) as u8;
    }
    // Try promoting from current: compare to threshold for (cur+1)-th bar.
    // For cur=0, promote to 1 when smoothed >= -85 + hysteresis.
    let next_thr = if cur < 3 { thr[cur] } else { -55 };
    if cur < 4 && smoothed >= next_thr + RSSI_HYSTERESIS_DB {
        return (cur + 1) as u8;
    }
    if cur > 0 {
        let cur_thr = thr[cur - 1]; // threshold that granted cur bars
        if smoothed < cur_thr - RSSI_HYSTERESIS_DB {
            return (cur - 1) as u8;
        }
    }
    current_bars
}

// ---- Per-peer tracker (D5, D6, D7) ---------------------------------------

#[derive(Debug, Clone)]
pub struct PeerStatus {
    pub mac: [u8; 6],
    pub online: bool,
    pub last_seen_ms: u64,
    pub rssi_ewma: i32,
    pub rssi_bars: u8,
}

impl PeerStatus {
    pub fn new(mac: [u8; 6]) -> Self {
        Self {
            mac,
            online: false,
            last_seen_ms: 0,
            rssi_ewma: -100,
            rssi_bars: 0,
        }
    }

    /// Update last_seen + RSSI on any received packet (D6).
    pub fn on_recv(&mut self, now_ms: u64, rssi: i32) {
        self.last_seen_ms = now_ms;
        self.online = true;
        // EWMA: new = α·raw + (1-α)·old
        self.rssi_ewma = (RSSI_EWMA_ALPHA_NUM * rssi
            + (RSSI_EWMA_ALPHA_DEN - RSSI_EWMA_ALPHA_NUM) * self.rssi_ewma)
            / RSSI_EWMA_ALPHA_DEN;
        self.rssi_bars = rssi_to_bars(self.rssi_ewma, self.rssi_bars);
    }

    /// Returns true if this peer just went offline (15s elapsed).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.online && now_ms - self.last_seen_ms >= OFFLINE_TIMEOUT.as_millis() as u64 {
            self.online = false;
            return true;
        }
        false
    }
}

// ---- HeartbeatTracker: manages all peers ---------------------------------

pub struct HeartbeatTracker {
    peers: Vec<PeerStatus>,
    last_heartbeat_sent_ms: u64,
    running: bool,
    /// Current IntercomState (for heartbeat period lookup).
    state: IntercomState,
    /// Whether the screen is currently off (extends period to 10s).
    screen_off: bool,
}

impl fmt::Debug for HeartbeatTracker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HeartbeatTracker")
            .field("peers", &self.peers.len())
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

impl HeartbeatTracker {
    pub fn new() -> Self {
        Self {
            peers: Vec::new(),
            last_heartbeat_sent_ms: 0,
            running: false,
            state: IntercomState::Idle,
            screen_off: false,
        }
    }

    /// Start tracking (called after restore_from_nvs succeeds).
    pub fn start(&mut self, now_ms: u64, group_peers: &[PeerEntry]) {
        self.peers.clear();
        for p in group_peers {
            self.peers.push(PeerStatus::new(p.mac));
        }
        self.last_heartbeat_sent_ms = now_ms;
        self.running = true;
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.peers.clear();
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Set the current IntercomState (task 2.5). Used for heartbeat period lookup.
    pub fn set_state(&mut self, state: IntercomState) {
        self.state = state;
    }

    /// Set whether the screen is off (task 2.6). Extends heartbeat period to 10s.
    pub fn set_screen_off(&mut self, off: bool) {
        self.screen_off = off;
    }

    pub fn state(&self) -> &IntercomState {
        &self.state
    }

    pub fn screen_off(&self) -> bool {
        self.screen_off
    }

    /// Returns true if a heartbeat should be sent now (task 2.3).
    /// Uses `heartbeat_period(state, screen_off)` for the period.
    pub fn should_send(&self, now_ms: u64, period: Duration) -> bool {
        self.running && now_ms - self.last_heartbeat_sent_ms >= period.as_millis() as u64
    }

    /// Returns true if a heartbeat should be sent now, using the internal state.
    pub fn should_send_now(&self, now_ms: u64) -> bool {
        let period = heartbeat_period(&self.state, self.screen_off);
        self.should_send(now_ms, period)
    }

    pub fn note_sent(&mut self, now_ms: u64) {
        self.last_heartbeat_sent_ms = now_ms;
    }

    /// Update a peer on any received packet (D6: any packet type counts).
    /// Returns a `PeerEvent::Online` if the peer just came back online (task 3.6).
    pub fn on_peer_recv(&mut self, mac: &[u8; 6], now_ms: u64, rssi: i32) -> Option<PeerEvent> {
        if let Some(p) = self.peers.iter_mut().find(|p| &p.mac == mac) {
            let was_offline = !p.online;
            p.on_recv(now_ms, rssi);
            if was_offline && p.online {
                return Some(PeerEvent::Online { mac: *mac, rssi_bars: p.rssi_bars });
            }
        }
        None
    }

    /// Tick all peers. Returns the list of macs that just went offline.
    pub fn tick(&mut self, now_ms: u64) -> Vec<[u8; 6]> {
        let mut newly_offline = Vec::new();
        for p in self.peers.iter_mut() {
            if p.tick(now_ms) {
                newly_offline.push(p.mac);
            }
        }
        newly_offline
    }

    pub fn peers(&self) -> &[PeerStatus] {
        &self.peers
    }

    pub fn peer_status(&self, mac: &[u8; 6]) -> Option<&PeerStatus> {
        self.peers.iter().find(|p| &p.mac == mac)
    }
}

/// Peer liveness events emitted to the UI/intercom layer (task 3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerEvent {
    /// Peer just came back online after being offline.
    Online { mac: [u8; 6], rssi_bars: u8 },
    /// Peer just went offline (15s timeout).
    Offline { mac: [u8; 6] },
}

impl Default for HeartbeatTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Restore orchestration outcome ---------------------------------------

/// Outcome of `restore_from_nvs()`. The caller executes these in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// NVS had no group or was schema-incompatible → clear + return to Idle.
    NoGroup,
    /// Group info loaded successfully; caller should init network at
    /// `channel`, re-derive LMKs, add_peer each, then enter Grouped(Idle).
    Restore {
        channel: u8,
        /// (mac, peer_pub_key) pairs to re-derive LMKs for.
        peers: Vec<([u8; 6], [u8; 32])>,
        mode_repr: u8,
    },
}

/// Decide what to do based on `Option<GroupInfo>`. `Some(group)` is what
/// `StorageService::load_group()` returned. If `None` or schema mismatch,
/// the caller should `clear_group()` and return to Idle.
pub fn plan_restore(loaded: Option<&GroupInfo>) -> RestoreOutcome {
    match loaded {
        None => RestoreOutcome::NoGroup,
        Some(g) => {
            let peers: Vec<([u8; 6], [u8; 32])> =
                g.peers.iter().map(|p| (p.mac, p.pub_key)).collect();
            RestoreOutcome::Restore {
                channel: g.channel,
                peers,
                mode_repr: g.mode as u8,
            }
        }
    }
}

// ---- IntercomService (task 4.1-4.5) --------------------------------------

/// Injected network send callback. Encodes a HEARTBEAT packet and sends it
/// to all peers via `NetworkService::send_unicast` (or broadcast). The
/// caller provides this because the encoding requires `packet::encode_heartbeat`
/// + `NetworkService` which are not owned by HeartbeatTracker.
pub trait HeartbeatSink: Send + Sync {
    /// Send a HEARTBEAT packet to all group peers.
    fn send_heartbeat(&self);
}

/// IntercomService coordinates restore + heartbeat + peer liveness.
/// Injected at construction with a `HeartbeatSink` for sending packets.
pub struct IntercomService {
    tracker: HeartbeatTracker,
    sink: Option<Box<dyn HeartbeatSink>>,
}

impl fmt::Debug for IntercomService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntercomService")
            .field("tracker", &self.tracker)
            .field("has_sink", &self.sink.is_some())
            .finish_non_exhaustive()
    }
}

impl IntercomService {
    pub fn new() -> Self {
        Self {
            tracker: HeartbeatTracker::new(),
            sink: None,
        }
    }

    /// Attach a heartbeat send callback (task 4.2).
    pub fn set_heartbeat_sink(&mut self, sink: Box<dyn HeartbeatSink>) {
        self.sink = Some(sink);
    }

    /// Restore from NVS (task 4.1: `restore_from_nvs`).
    /// Returns the restore outcome so the caller can execute the side-effecting
    /// steps (network init, derive LMKs, add_peer). On `Restore`, the service
    /// starts the heartbeat tracker with the loaded peers.
    pub fn restore_from_nvs(
        &mut self,
        now_ms: u64,
        loaded: Option<&GroupInfo>,
    ) -> RestoreOutcome {
        let outcome = plan_restore(loaded);
        match &outcome {
            RestoreOutcome::NoGroup => {
                // Stay Idle — no heartbeat.
            }
            RestoreOutcome::Restore { peers, .. } => {
                // Caller will execute network init + add_peer. We start the
                // tracker with the peer list so heartbeat tick begins.
                let peer_entries: Vec<PeerEntry> = peers
                    .iter()
                    .map(|(mac, pub_key)| PeerEntry { mac: *mac, pub_key: *pub_key })
                    .collect();
                self.tracker.start(now_ms, &peer_entries);
                self.tracker.set_state(IntercomState::Grouped(VoiceState::Idle));
            }
        }
        outcome
    }

    /// Leave the group (task 4.3): stop heartbeat + clear tracker.
    pub fn leave_group(&mut self) {
        self.tracker.stop();
    }

    /// Set the IntercomState (task 4.4: called at PTT/state-switch points).
    pub fn set_state(&mut self, state: IntercomState) {
        self.tracker.set_state(state);
    }

    /// Set screen off (task 4.4: called by power management).
    pub fn set_screen_off(&mut self, off: bool) {
        self.tracker.set_screen_off(off);
    }

    /// Called on any received ESP-NOW packet (task 3.2: update peer last_seen/
    /// online/rssi). Returns a `PeerEvent` if the peer just came online.
    pub fn on_recv(&mut self, mac: &[u8; 6], now_ms: u64, rssi: i32) -> Option<PeerEvent> {
        self.tracker.on_peer_recv(mac, now_ms, rssi)
    }

    /// Periodic tick (task 2.3: send heartbeat if due; task 3.5: mark offline).
    /// Returns the list of peers that just went offline.
    pub fn tick(&mut self, now_ms: u64) -> Vec<[u8; 6]> {
        // Send heartbeat if due.
        if self.tracker.should_send_now(now_ms) {
            if let Some(sink) = &self.sink {
                sink.send_heartbeat();
            }
            self.tracker.note_sent(now_ms);
        }
        // Mark offline peers.
        self.tracker.tick(now_ms)
    }

    pub fn tracker(&self) -> &HeartbeatTracker {
        &self.tracker
    }
}

impl Default for IntercomService {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Trait shims ---------------------------------------------------------

pub trait HeartbeatService: Send + Sync + fmt::Debug {
    fn tick(&self);
    fn on_heartbeat_recv(&self, src_mac: &[u8; 6]);
    fn last_seen(&self, src_mac: &[u8; 6]) -> Option<u64>;
}

pub trait RestoreService: Send + Sync + fmt::Debug {
    fn restore_last_state(&self) -> Option<u8>;
    fn save_current_state(&self, app_id: u8);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intercom::state::{HostPhase, IntercomMode, JoinPhase};
    use crate::services::storage::{GroupInfo, IntercomMode as StoredMode, PeerEntry};

    fn mk_group(channel: u8) -> GroupInfo {
        GroupInfo {
            schema_ver: 1,
            my_priv_key: [1u8; 32],
            peers: vec![PeerEntry { mac: [10, 20, 30, 40, 50, 60], pub_key: [0xCC; 32] }],
            mode: StoredMode::Clear,
            channel,
            last_state: 0,
        }
    }

    #[test]
    fn heartbeat_period_state_aware() {
        let s = IntercomState::Grouped(VoiceState::Idle);
        assert_eq!(heartbeat_period(&s, false), HB_PERIOD_IDLE);
        let s = IntercomState::Grouped(VoiceState::Talking);
        assert_eq!(heartbeat_period(&s, false), HB_PERIOD_VOICE);
        let s = IntercomState::Hosting(HostPhase::Discovering);
        assert_eq!(heartbeat_period(&s, false), HB_PERIOD_PAIRING);
        let s = IntercomState::Joining(JoinPhase::Searching);
        assert_eq!(heartbeat_period(&s, false), HB_PERIOD_PAIRING);
        // screen_off overrides to 10s regardless of VoiceState.
        let s = IntercomState::Grouped(VoiceState::Idle);
        assert_eq!(heartbeat_period(&s, true), HB_PERIOD_SCREEN_OFF);
    }

    #[test]
    fn rssi_bars_basic_mapping() {
        assert_eq!(rssi_to_bars_raw(-50), 4);
        assert_eq!(rssi_to_bars_raw(-60), 3);
        assert_eq!(rssi_to_bars_raw(-70), 2);
        assert_eq!(rssi_to_bars_raw(-80), 1);
        assert_eq!(rssi_to_bars_raw(-90), 0);
    }

    #[test]
    fn rssi_hysteresis_resists_jitter() {
        // Start at 4 bars (-50).
        let mut bars = rssi_to_bars(-50, 4);
        assert_eq!(bars, 4);
        // Jitter around -55 boundary shouldn't drop to 3 immediately.
        bars = rssi_to_bars(-57, bars); // -55 + (-3 hyst) = -58 to drop; -57 stays 4
        assert_eq!(bars, 4);
        // Drop below -55 - 3 = -58 → demote.
        bars = rssi_to_bars(-60, bars);
        assert_eq!(bars, 3);
    }

    #[test]
    fn peer_status_recv_updates_state() {
        let mut p = PeerStatus::new([1; 6]);
        assert!(!p.online);
        p.on_recv(100, -50);
        assert!(p.online);
        assert_eq!(p.last_seen_ms, 100);
        assert!(p.rssi_bars > 0);
    }

    #[test]
    fn peer_offline_after_15s() {
        let mut p = PeerStatus::new([1; 6]);
        p.on_recv(1000, -50);
        assert!(p.online);
        // 14s later — still online.
        assert!(!p.tick(15000));
        // 16s later — offline.
        assert!(p.tick(17000));
        assert!(!p.online);
    }

    #[test]
    fn tracker_starts_and_stops() {
        let mut t = HeartbeatTracker::new();
        let g = mk_group(11);
        t.start(0, &g.peers);
        assert!(t.running());
        assert_eq!(t.peers().len(), 1);
        t.stop();
        assert!(!t.running());
        assert_eq!(t.peers().len(), 0);
    }

    #[test]
    fn tracker_should_send_after_period() {
        let mut t = HeartbeatTracker::new();
        let g = mk_group(11);
        t.start(0, &g.peers);
        assert!(!t.should_send(1000, HB_PERIOD_IDLE)); // 1s < 5s
        assert!(t.should_send(5000, HB_PERIOD_IDLE));
        t.note_sent(5000);
        assert!(!t.should_send(9000, HB_PERIOD_IDLE));
        assert!(t.should_send(10000, HB_PERIOD_IDLE));
    }

    #[test]
    fn tracker_tick_marks_offline() {
        let mut t = HeartbeatTracker::new();
        let g = mk_group(11);
        t.start(0, &g.peers);
        let _ = t.on_peer_recv(&[10, 20, 30, 40, 50, 60], 100, -50);
        assert!(t.peers()[0].online);
        let offline = t.tick(20000); // 15s + 5s
        assert_eq!(offline, vec![[10, 20, 30, 40, 50, 60]]);
    }

    #[test]
    fn tracker_set_state_and_screen_off() {
        let mut t = HeartbeatTracker::new();
        t.set_state(IntercomState::Grouped(VoiceState::Talking));
        t.set_screen_off(true);
        assert_eq!(*t.state(), IntercomState::Grouped(VoiceState::Talking));
        assert!(t.screen_off());
        // screen_off overrides to 10s period regardless of Talking state.
        let period = heartbeat_period(t.state(), t.screen_off());
        assert_eq!(period, HB_PERIOD_SCREEN_OFF);
    }

    #[test]
    fn tracker_peer_online_event() {
        let mut t = HeartbeatTracker::new();
        let g = mk_group(11);
        t.start(0, &g.peers);
        // First recv — peer was offline → Online event.
        let evt = t.on_peer_recv(&[10, 20, 30, 40, 50, 60], 100, -50);
        assert!(matches!(evt, Some(PeerEvent::Online { .. })));
        // Second recv — peer already online → no event.
        let evt2 = t.on_peer_recv(&[10, 20, 30, 40, 50, 60], 200, -50);
        assert!(evt2.is_none());
    }

    #[test]
    fn intercom_service_restore_starts_tracker() {
        let mut svc = IntercomService::new();
        let g = mk_group(11);
        let outcome = svc.restore_from_nvs(0, Some(&g));
        match outcome {
            RestoreOutcome::Restore { channel, peers, .. } => {
                assert_eq!(channel, 11);
                assert_eq!(peers.len(), 1);
            }
            _ => panic!("expected Restore"),
        }
        assert!(svc.tracker().running());
        assert_eq!(svc.tracker().peers().len(), 1);
    }

    #[test]
    fn intercom_service_leave_stops_tracker() {
        let mut svc = IntercomService::new();
        let g = mk_group(11);
        svc.restore_from_nvs(0, Some(&g));
        svc.leave_group();
        assert!(!svc.tracker().running());
    }

    #[test]
    fn plan_restore_some_group() {
        let g = mk_group(11);
        let out = plan_restore(Some(&g));
        match out {
            RestoreOutcome::Restore { channel, peers, .. } => {
                assert_eq!(channel, 11);
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].0, [10, 20, 30, 40, 50, 60]);
            }
            _ => panic!("expected Restore"),
        }
    }

    #[test]
    fn plan_restore_none_returns_no_group() {
        let out = plan_restore(None);
        assert_eq!(out, RestoreOutcome::NoGroup);
    }
}
