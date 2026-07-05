//! Jitter buffer + multi-source mixer. Spec: §10.2, §10.5, §19.5. change 11.
//!
//! Pure-logic per-sender ring buffer with seq dedup + wraparound handling,
//! initial water level 3, dynamic max_water (floor 6 / hard cap 10), and a
//! fixed-attenuation mixer (0.7/route × cap 3) with soft limiter. Opus decode
//! is delegated to AudioService (change 05) — this module only buffers and
//! mixes PCM frames after decode.

#![allow(dead_code)]

use std::fmt;

use crate::board_profile::BoardProfile;
use crate::services::audio_service::{AudioFrame, PCM_SAMPLES_PER_FRAME};

/// Hard upper bound on per-sender ring capacity (10 frames / 200ms).
pub const RING_CAP: usize = 10;
/// Initial water level before starting pop_ready (3 frames / 60ms).
pub const INITIAL_WATER: usize = 3;
/// Floor for dynamic max_water.
pub const WATER_FLOOR: usize = 6;
/// Hard cap for dynamic max_water (== RING_CAP).
pub const WATER_HARD_CAP: usize = 10;
/// Max simultaneous routes mixed (PRD §13.2 cap 3).
pub const MIX_MAX_ROUTES: usize = 3;
/// PLC silence floor threshold (consecutive lost > 4 → silence).
pub const PLC_SILENCE_THRESHOLD: u8 = 4;
/// Clock-drift correction window (10s per D8).
pub const DRIFT_WINDOW_MS: u64 = 10_000;
/// Cooldown between drift corrections (10s per D8).
pub const DRIFT_COOLDOWN_MS: u64 = 10_000;
/// Low-water correction threshold: drop 1 frame when water < this.
pub const DRIFT_LOW_WATER: usize = 2;
/// High-water correction threshold: insert 1 PLC frame when water > this.
pub const DRIFT_HIGH_WATER: usize = 9;

/// Sentinel for "no seq seen yet" — ensures first seq=0 isn't dropped.
const SEQ_NONE: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy)]
pub struct FrameSlot {
    pub seq: u16,
    pub frame: Option<AudioFrame>,
}

impl Default for FrameSlot {
    fn default() -> Self {
        Self { seq: 0, frame: None }
    }
}

/// Result of pushing a frame into a ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushResult {
    /// Frame queued.
    Queued,
    /// Duplicate or out-of-order seq → dropped without queuing.
    DedupedDropped,
    /// Water exceeded max_water → oldest frame evicted to make room.
    EvictedOldest,
}

/// Single-sender jitter ring.
#[derive(Debug)]
pub struct JitterRing {
    slots: [FrameSlot; RING_CAP],
    head: usize,
    tail: usize,
    count: usize,
    last_seen_seq: u16,
    consecutive_lost: u8,
    /// Dynamic max water level (clamped to [WATER_FLOOR, WATER_HARD_CAP]).
    max_water: usize,
    /// Whether the initial water level has been reached (pop_ready active).
    primed: bool,
    /// EMA of observed water level peaks, used to adjust max_water.
    observed_ema: u32,
    /// 10s windowed water samples for clock-drift correction (D8).
    drift_samples: [(u64, usize); 32],
    drift_sample_count: usize,
    drift_last_correction_ms: u64,
}

impl Default for JitterRing {
    fn default() -> Self {
        Self::new()
    }
}

impl JitterRing {
    pub fn new() -> Self {
        Self {
            slots: [FrameSlot::default(); RING_CAP],
            head: 0,
            tail: 0,
            count: 0,
            last_seen_seq: SEQ_NONE,
            consecutive_lost: 0,
            max_water: WATER_FLOOR,
            primed: false,
            observed_ema: 0,
            drift_samples: [(0u64, 0usize); 32],
            drift_sample_count: 0,
            drift_last_correction_ms: 0,
        }
    }

    pub fn water_level(&self) -> usize {
        self.count
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    pub fn max_water(&self) -> usize {
        self.max_water
    }

    pub fn consecutive_lost(&self) -> u8 {
        self.consecutive_lost
    }

    /// Returns true if `seq` is a duplicate / out-of-order given `last_seen_seq`.
    /// Handles u16 wraparound: a diff > 32768 means wraparound, so the new
    /// seq is "newer" despite numeric comparison.
    fn is_stale(&self, seq: u16) -> bool {
        if self.last_seen_seq == SEQ_NONE {
            return false;
        }
        let last = self.last_seen_seq;
        // Wraparound: if last is near top and seq is near bottom, seq is newer.
        let diff = seq.wrapping_sub(last);
        // diff == 0 → exact duplicate. diff in 1..=32767 → newer. else stale.
        diff == 0 || (diff as i16 <= 0)
    }

    fn observe_peak(&mut self) {
        // Update EMA of observed peak water.
        let peak = self.count as u32;
        // EMA: new = old*7/8 + peak*1/8
        self.observed_ema = (self.observed_ema * 7 + peak * 8) / 8;
        // Recompute max_water = clamp(max(WATER_FLOOR, ema + 2), WATER_FLOOR, WATER_HARD_CAP)
        let target = (self.observed_ema / 8) as usize + 2;
        let new = target.clamp(WATER_FLOOR, WATER_HARD_CAP);
        self.max_water = new;
    }

    /// Sample water level for clock-drift correction (D8). Call on each tick.
    /// Records (now_ms, water_level) into a ring; when 10s windowed mean
    /// indicates persistent drift, returns a correction action.
    pub fn observe_drift(&mut self, now_ms: u64) -> DriftCorrection {
        if self.drift_sample_count < 32 {
            self.drift_samples[self.drift_sample_count] = (now_ms, self.count);
            self.drift_sample_count += 1;
        } else {
            // Ring-rewrite oldest.
            let idx = (now_ms / 312) as usize % 32;
            self.drift_samples[idx] = (now_ms, self.count);
        }
        // Cooldown: don't correct more than once per DRIFT_COOLDOWN_MS.
        if now_ms < self.drift_last_correction_ms + DRIFT_COOLDOWN_MS {
            return DriftCorrection::None;
        }
        // Compute 10s windowed mean water level.
        let window_start = now_ms.saturating_sub(DRIFT_WINDOW_MS);
        let mut sum: u64 = 0;
        let mut n: u64 = 0;
        for &(t, w) in self.drift_samples.iter() {
            if t >= window_start && t > 0 {
                sum += w as u64;
                n += 1;
            }
        }
        if n == 0 {
            return DriftCorrection::None;
        }
        let mean = (sum / n) as usize;
        if mean < DRIFT_LOW_WATER && self.count > 0 {
            // Low water: our clock is slow vs sender → drop 1 frame to catch up.
            self.drift_last_correction_ms = now_ms;
            self.tail = (self.tail + 1) % RING_CAP;
            self.count = self.count.saturating_sub(1);
            return DriftCorrection::DroppedFrame;
        }
        if mean > DRIFT_HIGH_WATER {
            // High water: our clock is fast vs sender → insert 1 PLC frame.
            self.drift_last_correction_ms = now_ms;
            return DriftCorrection::InsertPlc;
        }
        DriftCorrection::None
    }

    /// Push a frame. Returns PushResult.
    pub fn push(&mut self, seq: u16, frame: AudioFrame) -> PushResult {
        if self.is_stale(seq) {
            return PushResult::DedupedDropped;
        }
        self.last_seen_seq = seq;
        self.consecutive_lost = 0;

        // If water already at max_water, evict oldest.
        let mut evicted = false;
        if self.count >= self.max_water {
            // drop oldest
            self.tail = (self.tail + 1) % RING_CAP;
            self.count -= 1;
            evicted = true;
        }
        self.slots[self.head] = FrameSlot { seq, frame: Some(frame) };
        self.head = (self.head + 1) % RING_CAP;
        self.count += 1;
        if !self.primed && self.count >= INITIAL_WATER {
            self.primed = true;
        }
        self.observe_peak();
        if evicted {
            PushResult::EvictedOldest
        } else {
            PushResult::Queued
        }
    }

    /// Pop the oldest frame if primed.
    pub fn pop_ready(&mut self) -> Option<AudioFrame> {
        if !self.primed || self.count == 0 {
            return None;
        }
        let slot = &mut self.slots[self.tail];
        let frame = slot.frame.take();
        self.tail = (self.tail + 1) % RING_CAP;
        self.count -= 1;
        frame
    }

    /// Account for a timeout (no frame arrived this tick). Returns the PLC
    /// action to take: None (do PLC decode) or SilenceFloor (output zeros).
    pub fn note_timeout(&mut self) -> PlcAction {
        self.consecutive_lost = self.consecutive_lost.saturating_add(1);
        if self.consecutive_lost > PLC_SILENCE_THRESHOLD {
            self.consecutive_lost = 0;
            PlcAction::SilenceFloor
        } else {
            PlcAction::Predict
        }
    }

    /// Reset all state (used on group leave).
    pub fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            *s = FrameSlot::default();
        }
        self.head = 0;
        self.tail = 0;
        self.count = 0;
        self.last_seen_seq = SEQ_NONE;
        self.consecutive_lost = 0;
        self.max_water = WATER_FLOOR;
        self.primed = false;
        self.observed_ema = 0;
        self.drift_samples = [(0u64, 0usize); 32];
        self.drift_sample_count = 0;
        self.drift_last_correction_ms = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlcAction {
    /// Run libopus PLC (opus_decode(None)).
    Predict,
    /// Skip PLC, output zero PCM (consecutive_lost exceeded threshold).
    SilenceFloor,
}

/// Clock-drift correction action (D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftCorrection {
    /// No correction needed.
    None,
    /// Local clock slow vs sender — dropped 1 frame to catch up.
    DroppedFrame,
    /// Local clock fast vs sender — caller should insert 1 PLC frame.
    InsertPlc,
}

// ---- Multi-source manager -------------------------------------------------

/// Manages per-sender jitter rings + a mixer.
pub struct JitterMixer {
    rings: Vec<JitterRing>,
    /// Per-sender latest submitted PCM for mixing.
    pending: Vec<Option<[i16; PCM_SAMPLES_PER_FRAME]>>,
}

impl fmt::Debug for JitterMixer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JitterMixer")
            .field("senders", &self.rings.len())
            .finish_non_exhaustive()
    }
}

impl JitterMixer {
    pub fn new() -> Self {
        Self::with_capacity(BoardProfile::MAX_GROUP_SIZE as usize)
    }

    pub fn with_capacity(senders: usize) -> Self {
        let mut rings = Vec::with_capacity(senders);
        let mut pending = Vec::with_capacity(senders);
        for _ in 0..senders {
            rings.push(JitterRing::new());
            pending.push(None);
        }
        Self { rings, pending }
    }

    /// Push a frame for `sender_id`. Returns PushResult or error if sender_id
    /// is out of range.
    pub fn push(&mut self, sender_id: usize, seq: u16, frame: AudioFrame) -> Option<PushResult> {
        self.rings.get_mut(sender_id).map(|r| r.push(seq, frame))
    }

    /// Pop a ready frame from any primed ring, oldest-first by sender_id.
    pub fn pop_ready(&mut self) -> Option<(usize, AudioFrame)> {
        for i in 0..self.rings.len() {
            if let Some(f) = self.rings[i].pop_ready() {
                return Some((i, f));
            }
        }
        None
    }

    /// Note a tick timeout on a sender (no frame arrived). Returns PLC action.
    pub fn note_timeout(&mut self, sender_id: usize) -> Option<PlcAction> {
        self.rings.get_mut(sender_id).map(|r| r.note_timeout())
    }

    /// Submit decoded PCM for a sender (for mixer use).
    /// Per spec (task 6.1): the caller should submit this to AudioService
    /// via `AudioService::submit_pcm(src_id, pcm)`. This method only stores
    /// the PCM so `active_routes` can rank and return it.
    pub fn submit_pcm(&mut self, sender_id: usize, pcm: [i16; PCM_SAMPLES_PER_FRAME]) {
        if let Some(slot) = self.pending.get_mut(sender_id) {
            *slot = Some(pcm);
        }
    }

    /// Compute a resource-retention score for a sender (D7: stability ×
    /// duration). Stability = how often water level stayed above INITIAL_WATER
    /// (approximated by observed_ema). Duration = time since first push.
    /// Higher score = keep this route when truncating to MIX_MAX_ROUTES.
    fn route_score(&self, sender_id: usize) -> i32 {
        let ring = match self.rings.get(sender_id) {
            Some(r) => r,
            None => return -1,
        };
        let pcm_present = self.pending.get(sender_id).map_or(false, |s| s.is_some());
        if !pcm_present {
            return -1;
        }
        // Stability: observed_ema normalized to 0..100.
        let stability = (ring.observed_ema / 8) as i32;
        // Duration weight: more frames seen = higher. Use last_seen_seq as proxy.
        let duration = ring.last_seen_seq as i32;
        stability.saturating_add(duration / 100)
    }

    /// Return the active routes (sender_id, pcm) ranked by resource-retention
    /// score, capped at MIX_MAX_ROUTES. The caller submits each to
    /// `AudioService::submit_pcm(src_id, pcm)` — AudioService does the
    /// attenuation/sum/limit per spec (task 6.2). Clears pending after.
    pub fn active_routes(&mut self) -> Vec<(usize, [i16; PCM_SAMPLES_PER_FRAME])> {
        let mut scored: Vec<(i32, usize)> = (0..self.rings.len())
            .map(|i| (self.route_score(i), i))
            .filter(|(s, _)| *s >= 0)
            .collect();
        // Sort descending by score.
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.truncate(MIX_MAX_ROUTES);

        let mut out = Vec::with_capacity(scored.len());
        for (_, i) in &scored {
            if let Some(Some(pcm)) = self.pending.get(*i) {
                out.push((*i, *pcm));
            }
        }
        // Clear pending — caller is expected to submit fresh PCM each tick.
        for s in self.pending.iter_mut() {
            *s = None;
        }
        out
    }

    /// Observe clock drift on a sender's ring (D8). Call per-tick.
    pub fn observe_drift(&mut self, sender_id: usize, now_ms: u64) -> Option<DriftCorrection> {
        self.rings.get_mut(sender_id).map(|r| r.observe_drift(now_ms))
    }

    /// Reset all rings (group leave).
    pub fn reset(&mut self) {
        for r in self.rings.iter_mut() {
            r.reset();
        }
        for s in self.pending.iter_mut() {
            *s = None;
        }
    }

    pub fn water_level(&self, sender_id: usize) -> usize {
        self.rings.get(sender_id).map(|r| r.water_level()).unwrap_or(0)
    }
}

// ---- on_recv_voice main entry (task 10.1) --------------------------------

/// Outcome of `on_recv_voice`: the caller should decode the popped frame
/// (or run PLC) and submit the resulting PCM to AudioService via the
/// jitter mixer's `submit_pcm` + `active_routes`.
#[derive(Debug, Clone)]
pub struct RecvVoiceOutcome {
    /// (sender_id, frame) pairs ready for Opus decode.
    pub decode: Vec<(usize, AudioFrame)>,
    /// (sender_id, PlcAction) pairs needing PLC (opus_decode(None) or zeros).
    pub plc: Vec<(usize, PlcAction)>,
}

impl Default for RecvVoiceOutcome {
    fn default() -> Self {
        Self {
            decode: Vec::new(),
            plc: Vec::new(),
        }
    }
}

/// Main entry for received voice packet (task 10.1).
/// Pushes the frame into the sender's ring, then pops all ready frames
/// across all senders. The caller decodes each popped frame and submits
/// PCM via `JitterMixer::submit_pcm`, then calls `active_routes` to get
/// the final mix list for AudioService.
///
/// Per spec (task 5.5): jitter does NOT call opus_decode itself — the
/// caller does that via AudioService. This function only manages buffering.
pub fn on_recv_voice(
    mixer: &mut JitterMixer,
    sender_id: usize,
    seq: u16,
    frame: AudioFrame,
) -> RecvVoiceOutcome {
    let mut out = RecvVoiceOutcome::default();
    if mixer.push(sender_id, seq, frame).is_none() {
        // sender_id out of range — drop.
        return out;
    }
    // Pop all ready frames across all senders (oldest-first by sender_id).
    while let Some((sid, f)) = mixer.pop_ready() {
        out.decode.push((sid, f));
    }
    out
}

impl Default for JitterMixer {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Trait shims for callers that want dyn dispatch ----------------------

pub trait JitterBuffer: Send + Sync + fmt::Debug {
    fn push(&self, seq: u16, frame: AudioFrame);
    fn pop_ready(&self) -> Option<AudioFrame>;
    fn water_level(&self) -> u8;
}

pub trait Mixer: Send + Sync + fmt::Debug {
    fn submit(&self, src_id: u16, pcm: &[i16; PCM_SAMPLES_PER_FRAME]);
    fn mix(&self) -> [i16; PCM_SAMPLES_PER_FRAME];
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn mkframe(seq: u16, payload: u8) -> AudioFrame {
        let mut d = [0u8; crate::services::audio_service::MAX_OPUS_FRAME_SIZE];
        for b in d.iter_mut().take(8) {
            *b = payload;
        }
        AudioFrame { seq, opus_data: d, opus_len: 8 }
    }

    #[test]
    fn ring_dedups_duplicate_seq() {
        let mut r = JitterRing::new();
        assert_eq!(r.push(10, mkframe(10, 1)), PushResult::Queued);
        assert_eq!(r.push(10, mkframe(10, 2)), PushResult::DedupedDropped);
        assert_eq!(r.water_level(), 1);
    }

    #[test]
    fn ring_dedups_out_of_order() {
        let mut r = JitterRing::new();
        r.push(100, mkframe(100, 1));
        assert_eq!(r.push(50, mkframe(50, 2)), PushResult::DedupedDropped);
    }

    #[test]
    fn ring_handles_wraparound() {
        let mut r = JitterRing::new();
        r.push(0xFFFF, mkframe(0xFFFF, 1));
        // 0x0000 is "newer" after wraparound, must NOT be deduped.
        assert_eq!(r.push(0x0000, mkframe(0x0000, 2)), PushResult::Queued);
    }

    #[test]
    fn ring_initial_water_blocks_pop() {
        let mut r = JitterRing::new();
        r.push(1, mkframe(1, 1));
        r.push(2, mkframe(2, 1));
        assert_eq!(r.water_level(), 2);
        assert!(!r.is_primed());
        assert!(r.pop_ready().is_none());
        r.push(3, mkframe(3, 1));
        assert!(r.is_primed());
        assert!(r.pop_ready().is_some());
    }

    #[test]
    fn ring_evicts_oldest_at_max_water() {
        let mut r = JitterRing::new();
        // max_water starts at WATER_FLOOR (6). Push 6 then 1 more.
        for i in 1..=6 {
            assert_eq!(r.push(i, mkframe(i, 1)), PushResult::Queued);
        }
        // Prime happens at 3, water now 6 (== max_water).
        assert_eq!(r.push(7, mkframe(7, 1)), PushResult::EvictedOldest);
        assert_eq!(r.water_level(), 6);
        // Oldest (seq=1) evicted; pop should return seq=2.
        let popped = r.pop_ready().unwrap();
        assert_eq!(popped.seq, 2);
    }

    #[test]
    fn ring_plc_action_threshold() {
        let mut r = JitterRing::new();
        // First 4 timeouts → Predict
        for _ in 0..4 {
            assert_eq!(r.note_timeout(), PlcAction::Predict);
        }
        // 5th timeout → exceeds threshold → SilenceFloor + reset
        assert_eq!(r.note_timeout(), PlcAction::SilenceFloor);
        // After reset, next timeout is Predict again
        assert_eq!(r.note_timeout(), PlcAction::Predict);
    }

    #[test]
    fn ring_reset_clears_state() {
        let mut r = JitterRing::new();
        r.push(5, mkframe(5, 1));
        r.push(6, mkframe(6, 1));
        r.reset();
        assert_eq!(r.water_level(), 0);
        assert!(!r.is_primed());
        // After reset, last_seen_seq should be sentinel — seq=0 not stale.
        assert_eq!(r.push(0, mkframe(0, 1)), PushResult::Queued);
    }

    #[test]
    fn mixer_active_routes_returns_pending() {
        let mut m = JitterMixer::with_capacity(4);
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        for s in pcm.iter_mut() {
            *s = 30000;
        }
        m.submit_pcm(0, pcm);
        m.submit_pcm(1, pcm);
        m.submit_pcm(2, pcm);
        let routes = m.active_routes();
        // 3 routes returned (≤ MIX_MAX_ROUTES=3).
        assert_eq!(routes.len(), 3);
        // Per spec: jitter does NOT attenuate/sum/limit — caller (AudioService) does.
        // Each route's PCM is unchanged.
        assert_eq!(routes[0].1[0], 30000);
    }

    #[test]
    fn mixer_caps_routes_at_3() {
        let mut m = JitterMixer::with_capacity(4);
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        for s in pcm.iter_mut() {
            *s = 10000;
        }
        for i in 0..4 {
            m.submit_pcm(i, pcm);
        }
        let routes = m.active_routes();
        // 4 routes submitted but only 3 returned (MIX_MAX_ROUTES cap).
        assert_eq!(routes.len(), 3);
    }

    #[test]
    fn mixer_empty_returns_empty() {
        let mut m = JitterMixer::with_capacity(4);
        let routes = m.active_routes();
        assert!(routes.is_empty());
    }

    #[test]
    fn on_recv_voice_pops_ready_frames() {
        let mut m = JitterMixer::with_capacity(4);
        // Push 3 frames to sender 0 to prime the ring.
        for i in 1..=3 {
            let out = on_recv_voice(&mut m, 0, i, mkframe(i, 1));
            // Before priming, no frames are popped. After priming (3rd push),
            // the first frame is ready.
            if i >= 3 {
                assert!(!out.decode.is_empty());
            }
        }
    }

    #[test]
    fn jitter_mixer_routes_by_sender() {
        let mut m = JitterMixer::with_capacity(4);
        // Push 3 frames to sender 0 to prime
        for i in 1..=3 {
            m.push(0, i, mkframe(i, 1));
        }
        // Push 3 frames to sender 2 to prime
        for i in 1..=3 {
            m.push(2, i, mkframe(i, 2));
        }
        // Should be able to pop from sender 0 first
        let (sid, f) = m.pop_ready().unwrap();
        assert_eq!(sid, 0);
        assert_eq!(f.seq, 1);
        let (sid2, f2) = m.pop_ready().unwrap();
        assert_eq!(sid2, 0);
        assert_eq!(f2.seq, 2);
        // Then sender 2
        let (sid3, _) = m.pop_ready().unwrap();
        assert_eq!(sid3, 0);
        let (sid4, _) = m.pop_ready().unwrap();
        assert_eq!(sid4, 2);
    }
}
