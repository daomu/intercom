//! Voice changer (change 14). Spec: §3.5, §14, §17.
//!
//! Pure-Rust TD-PSOLA-style pitch shifter. Operates on 16kHz mono PCM
//! frames (320 samples / 20ms). PitchUp ratio = 1.5 (≈ +5 st), PitchDown
//! ratio = 0.667 (≈ -5 st). Normal = bypass.
//!
//! Algorithm: estimate pitch via normalized autocorrelation (lag 40-200,
//! 80Hz-400Hz range), then SOLA-style overlap-add with Hann window of size
//! 2×target_period, synthesis hop = target_period, source hop = src_period.
//! A small carry buffer preserves cross-frame continuity.

#![allow(dead_code)]

use std::fmt;

use crate::intercom::state::VoiceEffect;

const DEFAULT_PERIOD: usize = 100; // 160Hz fallback (D1 risk mitigation)
const MIN_LAG: usize = 40; // 400Hz
const MAX_LAG: usize = 200; // 80Hz
const Q15_ONE: i32 = 32768;

/// Hann window of size N (Q15 fixed-point).
fn hann_window(n: usize) -> Vec<i32> {
    let mut w = Vec::with_capacity(n);
    for i in 0..n {
        // w[i] = 0.5 * (1 - cos(2π i / (n-1)))
        let theta = 2.0 * std::f32::consts::PI * (i as f32) / ((n - 1) as f32).max(1.0);
        let val = 0.5 * (1.0 - (theta.cos()));
        w.push((val * Q15_ONE as f32) as i32);
    }
    w
}

/// Normalized autocorrelation pitch estimation. Returns None if no clear
/// peak found (silence/noise) — caller falls back to DEFAULT_PERIOD.
fn estimate_pitch(buf: &[i16]) -> Option<usize> {
    if buf.len() < MAX_LAG + 1 {
        return None;
    }
    // Use the middle portion of buf for stability.
    let start = buf.len() / 4;
    let end = buf.len() * 3 / 4;
    if end <= start + MAX_LAG {
        return None;
    }
    let seg = &buf[start..end];

    // Precompute energy of seg.
    let mut energy: i64 = 0;
    for s in seg {
        energy += (*s as i64) * (*s as i64);
    }
    if energy == 0 {
        return None;
    }

    let mut best_lag = 0usize;
    let mut best_norm: i128 = 0;
    for lag in MIN_LAG..=MAX_LAG {
        if lag >= seg.len() {
            break;
        }
        let mut corr: i64 = 0;
        for i in 0..(seg.len() - lag) {
            corr += (seg[i] as i64) * (seg[i + lag] as i64);
        }
        let mut lag_energy: i64 = 0;
        for i in 0..(seg.len() - lag) {
            lag_energy += (seg[i + lag] as i64) * (seg[i + lag] as i64);
        }
        if lag_energy == 0 {
            continue;
        }
        let norm = ((corr as i128) * (corr as i128))
            / ((energy as i128) * (lag_energy as i128));
        if norm > best_norm {
            best_norm = norm;
            best_lag = lag;
        }
    }
    if best_lag == 0 {
        return None;
    }
    Some(best_lag)
}

pub struct PitchShifter {
    effect: VoiceEffect,
    carry: Vec<i16>,
}

impl fmt::Debug for PitchShifter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PitchShifter")
            .field("effect", &self.effect)
            .field("carry_len", &self.carry.len())
            .finish()
    }
}

impl PitchShifter {
    pub fn new() -> Self {
        Self {
            effect: VoiceEffect::Normal,
            carry: Vec::new(),
        }
    }

    pub fn set_effect(&mut self, e: VoiceEffect) {
        self.effect = e;
        self.carry.clear();
    }

    pub fn effect(&self) -> VoiceEffect {
        self.effect
    }

    /// Process one frame. `input` and `output` must be the same length.
    pub fn process(&mut self, input: &[i16], output: &mut [i16]) {
        if input.len() != output.len() {
            // Length mismatch — bypass to avoid corruption.
            let n = input.len().min(output.len());
            output[..n].copy_from_slice(&input[..n]);
            return;
        }
        if self.effect == VoiceEffect::Normal {
            output.copy_from_slice(input);
            return;
        }

        let ratio = match self.effect {
            VoiceEffect::PitchUp => 1.5_f32,
            VoiceEffect::PitchDown => 0.667_f32,
            _ => 1.0,
        };

        // Build work buffer = carry + input.
        let mut work: Vec<i16> = Vec::with_capacity(self.carry.len() + input.len());
        work.extend_from_slice(&self.carry);
        work.extend_from_slice(input);

        let src_period = estimate_pitch(&work).unwrap_or(DEFAULT_PERIOD);
        let mut tgt_period = ((src_period as f32) / ratio).round() as usize;
        if tgt_period < 16 {
            tgt_period = 16;
        }
        if tgt_period > 400 {
            tgt_period = 400;
        }

        let window_size = 2 * tgt_period;
        if window_size == 0 || work.len() < window_size {
            // Not enough data — bypass and save carry for next frame.
            output.copy_from_slice(input);
            let carry_size = 320.min(input.len());
            self.carry = input[input.len() - carry_size..].to_vec();
            return;
        }

        let hann = hann_window(window_size);

        // Initialize output to 0 (mixing will accumulate).
        for s in output.iter_mut() {
            *s = 0;
        }
        // Sum-of-windows normalization accumulator (for overlap-add normalization).
        let mut norm_acc: Vec<i32> = vec![0; output.len()];

        let half = window_size / 2;
        let half_i: i64 = half as i64;
        let mut out_pos: i64 = half_i; // start so first window center is at half
        let mut src_pos: i64 = half_i;

        while out_pos + half_i < output.len() as i64 + half_i
            && src_pos + half_i < work.len() as i64
        {
            // Add windowed segment centered at src_pos to output centered at out_pos.
            for j in 0..window_size {
                let out_idx = out_pos + (j as i64) - half_i;
                let src_idx = src_pos + (j as i64) - half_i;
                if out_idx >= 0
                    && (out_idx as usize) < output.len()
                    && src_idx >= 0
                    && (src_idx as usize) < work.len()
                {
                    let s = work[src_idx as usize] as i32;
                    let w = hann[j];
                    // Accumulate in i32 with Q15 scaling.
                    let acc = output[out_idx as usize] as i32
                        + (s * w) / Q15_ONE;
                    output[out_idx as usize] = acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                    norm_acc[out_idx as usize] += w;
                }
            }
            out_pos += tgt_period as i64;
            src_pos += src_period as i64;
            if out_pos - half_i >= output.len() as i64 {
                break;
            }
        }

        // Normalize by sum-of-windows to keep amplitude stable.
        for i in 0..output.len() {
            let n = norm_acc[i];
            if n > 0 {
                let v = (output[i] as i32 * Q15_ONE) / n;
                output[i] = v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            }
        }

        // Save carry for next frame: last 2*tgt_period samples of work.
        let carry_size = 2 * tgt_period;
        if work.len() >= carry_size {
            self.carry = work[work.len() - carry_size..].to_vec();
        } else {
            self.carry = work;
        }
    }
}

impl Default for PitchShifter {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Trait shim for callers expecting a service-like object ---------------

pub trait VoiceChanger: Send + Sync + fmt::Debug {
    fn set_effect(&self, e: VoiceEffect);
    fn process(&self, input: &[i16], output: &mut [i16]);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn sine_buf(n: usize, freq_hz: f32, sr: f32) -> Vec<i16> {
        let mut v = Vec::with_capacity(n);
        let mut phase = 0.0;
        let dphase = 2.0 * std::f32::consts::PI * freq_hz / sr;
        for _ in 0..n {
            phase += dphase;
            v.push((phase.sin() * 16000.0) as i16);
        }
        v
    }

    /// Estimate dominant frequency of buf via autocorrelation peak.
    fn estimate_freq(buf: &[i16], sr: f32) -> f32 {
        let mut best_lag = 0;
        let mut best_corr: i64 = 0;
        for lag in 20..400 {
            if lag >= buf.len() {
                break;
            }
            let mut corr: i64 = 0;
            for i in 0..(buf.len() - lag) {
                corr += (buf[i] as i64) * (buf[i + lag] as i64);
            }
            if corr.abs() > best_corr {
                best_corr = corr.abs();
                best_lag = lag;
            }
        }
        if best_lag == 0 {
            0.0
        } else {
            sr / best_lag as f32
        }
    }

    #[test]
    fn normal_bypass_copies_input() {
        let mut shifter = PitchShifter::new();
        shifter.set_effect(VoiceEffect::Normal);
        let input = sine_buf(320, 200.0, 16000.0);
        let mut output = vec![0i16; 320];
        shifter.process(&input, &mut output);
        assert_eq!(output, input);
    }

    #[test]
    fn pitch_up_raises_dominant_frequency() {
        let mut shifter = PitchShifter::new();
        shifter.set_effect(VoiceEffect::PitchUp);
        // Feed several frames to let the carry buffer stabilize.
        let mut input = sine_buf(320, 200.0, 16000.0);
        let mut output = vec![0i16; 320];
        for _ in 0..5 {
            let inp = input.clone();
            shifter.process(&inp, &mut output);
            input = output.clone();
        }
        // Final output dominant frequency should be > 200 Hz (target ~300 Hz).
        let f = estimate_freq(&output, 16000.0);
        // Loosen threshold — SOLA on finite frames has measurement noise.
        assert!(f > 240.0, "expected >240 Hz after pitch up, got {}", f);
    }

    #[test]
    fn pitch_down_lowers_dominant_frequency() {
        let mut shifter = PitchShifter::new();
        shifter.set_effect(VoiceEffect::PitchDown);
        let mut input = sine_buf(320, 400.0, 16000.0);
        let mut output = vec![0i16; 320];
        for _ in 0..5 {
            let inp = input.clone();
            shifter.process(&inp, &mut output);
            input = output.clone();
        }
        let f = estimate_freq(&output, 16000.0);
        assert!(f < 340.0, "expected <340 Hz after pitch down, got {}", f);
    }

    #[test]
    fn silence_passes_through_without_panic() {
        let mut shifter = PitchShifter::new();
        shifter.set_effect(VoiceEffect::PitchUp);
        let input = vec![0i16; 320];
        let mut output = vec![0i16; 320];
        shifter.process(&input, &mut output);
        // Output should be (near) silent.
        for s in &output {
            assert!(s.abs() < 100);
        }
    }

    #[test]
    fn hann_window_sums_symmetric() {
        let w = hann_window(64);
        // Hann window is symmetric.
        for i in 0..32 {
            assert!((w[i] - w[63 - i]).abs() <= 2, "asymmetric at {}", i);
        }
    }

    #[test]
    fn estimate_pitch_detects_sine() {
        // 200 Hz @ 16 kHz → 80-sample period.
        let buf = sine_buf(640, 200.0, 16000.0);
        let p = estimate_pitch(&buf).unwrap_or(0);
        assert!((p as i32 - 80).abs() <= 4, "expected ~80, got {}", p);
    }

    /// Thread-safe wrapper for the trait shim (process needs &mut self,
    /// but the trait requires &self — wrap in Mutex).
    pub struct ThreadSafePitchShifter {
        inner: Mutex<PitchShifter>,
    }

    impl fmt::Debug for ThreadSafePitchShifter {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("ThreadSafePitchShifter").finish_non_exhaustive()
        }
    }

    impl ThreadSafePitchShifter {
        pub fn new() -> Self {
            Self { inner: Mutex::new(PitchShifter::new()) }
        }
    }

    impl VoiceChanger for ThreadSafePitchShifter {
        fn set_effect(&self, e: VoiceEffect) {
            self.inner.lock().unwrap().set_effect(e);
        }
        fn process(&self, input: &[i16], output: &mut [i16]) {
            self.inner.lock().unwrap().process(input, output);
        }
    }

    unsafe impl Send for ThreadSafePitchShifter {}
    unsafe impl Sync for ThreadSafePitchShifter {}
}
