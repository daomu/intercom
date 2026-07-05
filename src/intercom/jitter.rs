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
/// Per-route attenuation (PRD §13.2 / design D6).
pub const MIX_ATTEN: i32 = 7; // /10
/// Max simultaneous routes mixed (PRD §13.2 cap 3).
pub const MIX_MAX_ROUTES: usize = 3;
/// PLC silence floor threshold (consecutive lost > 4 → silence).
pub const PLC_SILENCE_THRESHOLD: u8 = 4;

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
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlcAction {
    /// Run libopus PLC (opus_decode(None)).
    Predict,
    /// Skip PLC, output zero PCM (consecutive_lost exceeded threshold).
    SilenceFloor,
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
    pub fn submit_pcm(&mut self, sender_id: usize, pcm: [i16; PCM_SAMPLES_PER_FRAME]) {
        if let Some(slot) = self.pending.get_mut(sender_id) {
            *slot = Some(pcm);
        }
    }

    /// Mix the currently-pending PCM streams with fixed 0.7 attenuation per
    /// route, capped at MIX_MAX_ROUTES. Saturating soft-limiter clips to i16
    /// range. Clears pending after mixing.
    pub fn mix(&mut self) -> [i16; PCM_SAMPLES_PER_FRAME] {
        // Collect non-empty routes; if more than MIX_MAX_ROUTES, keep first 3
        // (callers should pre-rank, but as a guard we just truncate).
        let mut routes: Vec<&[i16; PCM_SAMPLES_PER_FRAME]> = self
            .pending
            .iter()
            .filter_map(|s| s.as_ref())
            .collect();
        if routes.len() > MIX_MAX_ROUTES {
            routes.truncate(MIX_MAX_ROUTES);
        }

        let mut out = [0i16; PCM_SAMPLES_PER_FRAME];
        if routes.is_empty() {
            return out;
        }
        let denom = 10i32;
        for i in 0..PCM_SAMPLES_PER_FRAME {
            let mut acc: i32 = 0;
            for r in &routes {
                acc += (*r)[i] as i32 * MIX_ATTEN / denom;
            }
            // Soft limiter: tanh-like via clamp at ±30000 then i16 clamp.
            let clamped = acc.clamp(-30000, 30000);
            out[i] = clamped.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
        // Clear pending — caller is expected to submit fresh PCM each tick.
        for s in self.pending.iter_mut() {
            *s = None;
        }
        out
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
    fn mixer_attenuates_and_clips() {
        let mut m = JitterMixer::with_capacity(4);
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        for s in pcm.iter_mut() {
            *s = 30000;
        }
        m.submit_pcm(0, pcm);
        m.submit_pcm(1, pcm);
        m.submit_pcm(2, pcm);
        let mixed = m.mix();
        // 3 routes × 30000 × 0.7 = 63000 → clamped to 30000 (soft limiter)
        assert_eq!(mixed[0], 30000);
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
        let mixed = m.mix();
        // 4 routes submitted but only 3 mixed: 3 × 10000 × 0.7 = 21000
        assert_eq!(mixed[0], 21000);
    }

    #[test]
    fn mixer_empty_returns_zeros() {
        let mut m = JitterMixer::with_capacity(4);
        let mixed = m.mix();
        assert_eq!(mixed[0], 0);
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
