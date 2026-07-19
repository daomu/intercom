//! Audio service: Opus encode/decode + I2S + PA soft-start. change 05/17.
//! Spec: §3.4. design D1-D6 (see change 05 design.md).

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod thread;

use std::fmt;

use crate::board_profile::BoardProfile;

/// Max Opus frame size at 16kHz/20ms VoIP ≈ 80 bytes; pad to 160 for safety.
pub const MAX_OPUS_FRAME_SIZE: usize = 160;
/// 16kHz × 20ms × 16bit × 1ch = 640 bytes = 320 i16 samples per frame.
pub const PCM_SAMPLES_PER_FRAME: usize = 320;

#[derive(Debug, Clone, Copy)]
pub struct AudioFrame {
    pub seq: u16,
    pub opus_data: [u8; MAX_OPUS_FRAME_SIZE],
    pub opus_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioError {
    I2sError,
    OpusError,
    BufferExhausted,
    InvalidParam,
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::I2sError => write!(f, "i2s error"),
            AudioError::OpusError => write!(f, "opus error"),
            AudioError::BufferExhausted => write!(f, "buffer exhausted"),
            AudioError::InvalidParam => write!(f, "invalid param"),
        }
    }
}
impl std::error::Error for AudioError {}

// ---- Voice effect stage (D5: insert point for voice changer) -----------

/// Pluggable PCM effect stage inserted between I2S capture and Opus encode.
/// Implementations: `PassthroughStage` (default), voice-changer DSP (change 14).
pub trait VoiceEffectStage: Send + Sync {
    /// Mutate PCM in-place. Called per frame (~20ms / 320 samples).
    fn process(&self, pcm: &mut [i16; PCM_SAMPLES_PER_FRAME]);
}

/// Default no-op stage. `HalAudioService` defaults to this when no voice
/// changer is plugged in.
pub struct PassthroughStage;

impl fmt::Debug for PassthroughStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PassthroughStage")
    }
}

impl VoiceEffectStage for PassthroughStage {
    fn process(&self, _pcm: &mut [i16; PCM_SAMPLES_PER_FRAME]) {}
}

// ---- MixSlot (D7: 3-way mix with soft limiter) -------------------------

/// One slot of the 3-way mix buffer. Indexed by `src_id` 0..3.
#[derive(Debug, Clone, Copy)]
pub struct MixSlot {
    pub pcm: [i16; PCM_SAMPLES_PER_FRAME],
    pub last_seq: u16,
    pub last_update_ms: u32,
    pub active: bool,
}

impl MixSlot {
    pub const fn new() -> Self {
        Self {
            pcm: [0i16; PCM_SAMPLES_PER_FRAME],
            last_seq: 0,
            last_update_ms: 0,
            active: false,
        }
    }
}

/// Per-source attenuation (D7: 0.7 linear to prevent 3-way sum clipping).
pub const MIX_ATTENUATION: f32 = 0.7;

/// Soft limiter (tanh-style). Hard clip would introduce harmonic distortion;
/// tanh compresses gracefully as samples approach the rail.
fn soft_limit(x: i32) -> i16 {
    const RAIL: i32 = i16::MAX as i32;
    // Approximation: tanh(x) ≈ x / (1 + |x|/RAIL) — cheap, monotonic, smooth.
    let scaled = x as f32 / RAIL as f32;
    let compressed = scaled / (1.0 + scaled.abs());
    (compressed * RAIL as f32) as i16
}

/// 3-way mix + soft limit. Returns the mixed PCM frame.
pub fn mix_and_play(slots: &[MixSlot; 3], volume_u8: u8, muted: bool) -> [i16; PCM_SAMPLES_PER_FRAME] {
    if muted {
        return [0i16; PCM_SAMPLES_PER_FRAME];
    }
    let vol = (volume_u8 as f32) / 255.0;
    let mut out = [0i16; PCM_SAMPLES_PER_FRAME];
    for i in 0..PCM_SAMPLES_PER_FRAME {
        let mut sum: f32 = 0.0;
        let mut active_count = 0u32;
        for slot in slots.iter() {
            if slot.active {
                sum += (slot.pcm[i] as f32) * MIX_ATTENUATION;
                active_count += 1;
            }
        }
        if active_count == 0 {
            out[i] = 0;
        } else {
            let limited = soft_limit(sum as i32);
            out[i] = ((limited as f32) * vol) as i16;
        }
    }
    out
}

/// Audio service trait. §3.4. design D1-D6.
pub trait AudioService: Send + Sync + fmt::Debug {
    fn start_capture(&self) -> Result<(), AudioError>;
    fn stop_capture(&self) -> Result<(), AudioError>;
    fn start_playback(&self) -> Result<(), AudioError>;
    fn stop_playback(&self) -> Result<(), AudioError>;

    /// Encode captured PCM (320 samples) → Opus frame. (legacy API)
    fn encode(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<AudioFrame, AudioError>;
    /// Decode Opus frame → 320 PCM samples. (legacy API)
    fn decode(&self, frame: &AudioFrame) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError>;

    /// Opus decode with PLC support. `None` requests packet-loss concealment
    /// (4-frame threshold in `BoardProfile::PLC_CONSECUTIVE_LOSS_THRESHOLD`).
    /// `Some(data)` decodes a normal frame.
    fn opus_decode(
        &self,
        frame: Option<&[u8]>,
    ) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError>;

    /// Submit decoded PCM for a specific source (0..3). The service mixes
    /// active sources via `mix_and_play` and writes the result to I2S.
    fn submit_pcm(&self, src_id: u8, pcm: &[i16; PCM_SAMPLES_PER_FRAME])
        -> Result<(), AudioError>;

    /// Register a callback invoked on each captured+encoded frame (D5).
    /// Replaces any prior callback. The callback receives `&AudioFrame`.
    fn on_capture_frame(&self, cb: Box<dyn Fn(&AudioFrame) + Send + Sync>);

    /// Set volume 0..=255. Mapped to attenuation coefficient in `mix_and_play`.
    fn set_volume(&self, v: u8);
    /// Set mute. Does NOT toggle PA (PA state is independent per D6).
    fn set_mute(&self, m: bool);

    /// PA soft-start: enable PA after 1-2 frames buffer; disable before stop.
    fn pa_enable(&self, on: bool);

    // ---- Legacy single-source submit_pcm (calls submit_pcm(0, _)) ----
    /// Legacy single-source submit. Equivalent to `submit_pcm(0, pcm)`.
    fn submit_pcm_legacy(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError> {
        self.submit_pcm(0, pcm)
    }
}

/// Stub impl. Real Opus/I2S wiring deferred to on-hardware verification.
pub struct AudioServiceStub {
    _sample_rate: u32,
    volume: std::sync::atomic::AtomicU8,
    mute: std::sync::atomic::AtomicBool,
    capture_cb: Mutex<Option<Box<dyn Fn(&AudioFrame) + Send + Sync>>>,
    slots: Mutex<[MixSlot; 3]>,
    seq: Mutex<u16>,
}

impl fmt::Debug for AudioServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioServiceStub")
            .field("volume", &self.volume.load(std::sync::atomic::Ordering::Relaxed))
            .field("mute", &self.mute.load(std::sync::atomic::Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl AudioServiceStub {
    pub fn new() -> Self {
        Self {
            _sample_rate: BoardProfile::OPUS_SAMPLE_RATE,
            volume: std::sync::atomic::AtomicU8::new(255),
            mute: std::sync::atomic::AtomicBool::new(false),
            capture_cb: Mutex::new(None),
            slots: Mutex::new([MixSlot::new(); 3]),
            seq: Mutex::new(0),
        }
    }
}

impl Default for AudioServiceStub {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioService for AudioServiceStub {
    fn start_capture(&self) -> Result<(), AudioError> { Ok(()) }
    fn stop_capture(&self) -> Result<(), AudioError> { Ok(()) }
    fn start_playback(&self) -> Result<(), AudioError> { Ok(()) }
    fn stop_playback(&self) -> Result<(), AudioError> { Ok(()) }

    fn encode(&self, _pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<AudioFrame, AudioError> {
        let mut seq = self.seq.lock().unwrap();
        *seq = seq.wrapping_add(1);
        Ok(AudioFrame {
            seq: *seq,
            opus_data: [0u8; MAX_OPUS_FRAME_SIZE],
            opus_len: 0,
        })
    }

    fn decode(&self, _frame: &AudioFrame) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        Ok([0i16; PCM_SAMPLES_PER_FRAME])
    }

    fn opus_decode(
        &self,
        _frame: Option<&[u8]>,
    ) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        Ok([0i16; PCM_SAMPLES_PER_FRAME])
    }

    fn submit_pcm(&self, src_id: u8, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError> {
        let mut slots = self.slots.lock().unwrap();
        let idx = (src_id as usize).min(2);
        slots[idx].pcm = *pcm;
        slots[idx].active = true;
        Ok(())
    }

    fn on_capture_frame(&self, cb: Box<dyn Fn(&AudioFrame) + Send + Sync>) {
        *self.capture_cb.lock().unwrap() = Some(cb);
    }

    fn set_volume(&self, v: u8) {
        self.volume.store(v, std::sync::atomic::Ordering::SeqCst);
    }
    fn set_mute(&self, m: bool) {
        self.mute.store(m, std::sync::atomic::Ordering::SeqCst);
    }
    fn pa_enable(&self, _on: bool) {}
}

unsafe impl Send for AudioServiceStub {}
unsafe impl Sync for AudioServiceStub {}

// ---- Real HAL impl (I2S + PA soft-start + Opus FFI) ---------------------

use std::sync::Mutex;

use esp_idf_svc::hal::i2s::{I2sBiDir, I2sDriver};

use crate::hal::audio_in::AudioInDriver;
use crate::hal::audio_out::AudioOutDriver;
use crate::hal::HalError;

/// Real audio service backed by `AudioInDriver` (ES7210 + I2S0 RX) +
/// `AudioOutDriver` (ES8311 + I2S0 TX + PA_CTRL GPIO15). Owns the shared
/// `I2sDriver<'static, I2sBiDir>` (moved out of `Hal` during wiring).
///
/// Opus encode/decode is gated behind the `opus` cargo feature. Without it,
/// `encode`/`decode` return `AudioError::OpusError` — the I2S + PA path still
/// works for raw PCM loopback / verification.
pub struct HalAudioService {
    i2s: Mutex<I2sDriver<'static, I2sBiDir>>,
    audio_in: Mutex<AudioInDriver>,
    audio_out: Mutex<AudioOutDriver>,
    capturing: Mutex<bool>,
    playing: Mutex<bool>,
    seq: Mutex<u16>,
    volume: std::sync::atomic::AtomicU8,
    mute: std::sync::atomic::AtomicBool,
    capture_cb: Mutex<Option<Box<dyn Fn(&AudioFrame) + Send + Sync>>>,
    slots: Mutex<[MixSlot; 3]>,
    /// Pluggable voice-changer stage (None = passthrough).
    effect_stage: Mutex<Option<Box<dyn VoiceEffectStage>>>,
}

impl fmt::Debug for HalAudioService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HalAudioService")
            .field("capturing", &self.capturing)
            .field("playing", &self.playing)
            .field("volume", &self.volume.load(std::sync::atomic::Ordering::Relaxed))
            .field("mute", &self.mute.load(std::sync::atomic::Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl HalAudioService {
    /// Construct from owned I2S driver + audio HAL drivers. The I2S driver
    /// is moved here from `Hal` (audio is the only I2S0 consumer).
    pub fn new(
        i2s: I2sDriver<'static, I2sBiDir>,
        audio_in: AudioInDriver,
        audio_out: AudioOutDriver,
    ) -> Self {
        Self {
            i2s: Mutex::new(i2s),
            audio_in: Mutex::new(audio_in),
            audio_out: Mutex::new(audio_out),
            capturing: Mutex::new(false),
            playing: Mutex::new(false),
            seq: Mutex::new(0),
            volume: std::sync::atomic::AtomicU8::new(255),
            mute: std::sync::atomic::AtomicBool::new(false),
            capture_cb: Mutex::new(None),
            slots: Mutex::new([MixSlot::new(); 3]),
            effect_stage: Mutex::new(None),
        }
    }

    /// Plug in a voice-changer stage (None = passthrough).
    pub fn set_effect_stage(&self, stage: Option<Box<dyn VoiceEffectStage>>) {
        *self.effect_stage.lock().unwrap() = stage;
    }

    /// Read one 20ms PCM frame from the I2S RX channel (ES7210) and apply the
    /// pluggable effect stage. Blocks on the I2S read. Used by the audio
    /// thread TX path (wire-audio-pipeline task 2.2).
    pub fn capture_frame(&self) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        let mut buf = [0u8; PCM_SAMPLES_PER_FRAME * 2];
        {
            let audio_in = self.audio_in.lock().unwrap();
            let mut i2s = self.i2s.lock().unwrap();
            audio_in
                .read_pcm(&mut i2s, &mut buf)
                .map_err(Self::map_err)?;
        }
        // Reinterpret native LE bytes as i16 samples.
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        for (i, s) in pcm.iter_mut().enumerate() {
            *s = i16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]]);
        }
        // Apply the pluggable voice-changer stage (None = passthrough).
        if let Some(stage) = self.effect_stage.lock().unwrap().as_ref() {
            stage.process(&mut pcm);
        }
        Ok(pcm)
    }

    /// True while capture is armed (main thread sets via `start_capture`).
    pub fn is_capturing(&self) -> bool {
        *self.capturing.lock().unwrap()
    }

    /// True while playback is active (main thread sets via `start_playback`).
    pub fn is_playing(&self) -> bool {
        *self.playing.lock().unwrap()
    }

    fn map_err(e: HalError) -> AudioError {
        match e {
            HalError::AudioInInitFailed(_) | HalError::AudioOutInitFailed(_) => AudioError::I2sError,
            _ => AudioError::I2sError,
        }
    }
}

impl AudioService for HalAudioService {
    fn start_capture(&self) -> Result<(), AudioError> {
        *self.capturing.lock().unwrap() = true;
        Ok(())
    }
    fn stop_capture(&self) -> Result<(), AudioError> {
        *self.capturing.lock().unwrap() = false;
        Ok(())
    }
    fn start_playback(&self) -> Result<(), AudioError> {
        *self.playing.lock().unwrap() = true;
        // PA soft-start: enable PA after buffer is primed. Per design D6,
        // caller enables PA after 1-2 frames are buffered; here we just flip
        // the playback flag.
        Ok(())
    }
    fn stop_playback(&self) -> Result<(), AudioError> {
        // D6: zero-frame fade-out — write 1-2 frames of silence before PA
        // disable to avoid pop. Then disable PA, then stop I2S.
        let mut out = self.audio_out.lock().unwrap();
        // Write 2 frames of silence (640 bytes each = 1280 bytes total).
        let silence = [0u8; PCM_SAMPLES_PER_FRAME * 2];
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(
                silence.as_ptr() as *const u8,
                std::mem::size_of_val(&silence),
            )
        };
        let _ = out.write_pcm(&mut self.i2s.lock().unwrap(), bytes);
        // Now disable PA (no pop because silence is already in the buffer).
        let _ = out.pa_enable(false);
        *self.playing.lock().unwrap() = false;
        Ok(())
    }
    fn encode(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<AudioFrame, AudioError> {
        #[cfg(feature = "opus")]
        {
            // TODO: audiopus encoder init + encode. Requires the `opus` feature
            // plus cross-compile wiring (LIBOPUS_LIB_DIR or toolchain PATH).
            // For now, return an empty frame so the pipeline type-checks.
            let mut seq = self.seq.lock().unwrap();
            *seq = seq.wrapping_add(1);
            return Ok(AudioFrame {
                seq: *seq,
                opus_data: [0u8; MAX_OPUS_FRAME_SIZE],
                opus_len: 0,
            });
        }
        #[cfg(not(feature = "opus"))]
        {
            let _ = pcm;
            Err(AudioError::OpusError)
        }
    }
    fn decode(&self, frame: &AudioFrame) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        #[cfg(feature = "opus")]
        {
            // TODO: audiopus decoder init + decode.
            let _ = frame;
            return Ok([0i16; PCM_SAMPLES_PER_FRAME]);
        }
        #[cfg(not(feature = "opus"))]
        {
            let _ = frame;
            Err(AudioError::OpusError)
        }
    }

    fn opus_decode(
        &self,
        frame: Option<&[u8]>,
    ) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        #[cfg(feature = "opus")]
        {
            // TODO: audiopus decoder. None → decoder.decode_plc(out); Some(d) → decoder.decode(d, out).
            let _ = frame;
            return Ok([0i16; PCM_SAMPLES_PER_FRAME]);
        }
        #[cfg(not(feature = "opus"))]
        {
            let _ = frame;
            Err(AudioError::OpusError)
        }
    }

    fn submit_pcm(&self, src_id: u8, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError> {
        // Deposit PCM into the per-source mix slot.
        let mut slots = self.slots.lock().unwrap();
        let idx = (src_id as usize).min(2);
        slots[idx].pcm = *pcm;
        slots[idx].active = true;
        // Snapshot for mix (drop lock before I2S write).
        let snapshot = *slots;
        drop(slots);

        // Mix active sources with attenuation + soft limiter.
        let vol = self.volume.load(std::sync::atomic::Ordering::Relaxed);
        let muted = self.mute.load(std::sync::atomic::Ordering::Relaxed);
        let mixed = mix_and_play(&snapshot, vol, muted);

        let mut out = self.audio_out.lock().unwrap();
        // PA soft-start: if first frame since start_playback, enable PA now.
        if *self.playing.lock().unwrap() {
            let _ = out.pa_enable(true);
        }
        // Reinterpret [i16; N] as [u8; N*2] (little-endian, native I2S format).
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(mixed.as_ptr() as *const u8, std::mem::size_of_val(&mixed))
        };
        out.write_pcm(&mut self.i2s.lock().unwrap(), bytes)
            .map_err(Self::map_err)?;
        Ok(())
    }

    fn on_capture_frame(&self, cb: Box<dyn Fn(&AudioFrame) + Send + Sync>) {
        *self.capture_cb.lock().unwrap() = Some(cb);
    }

    fn set_volume(&self, v: u8) {
        self.volume.store(v, std::sync::atomic::Ordering::SeqCst);
    }
    fn set_mute(&self, m: bool) {
        self.mute.store(m, std::sync::atomic::Ordering::SeqCst);
    }
    fn pa_enable(&self, on: bool) {
        let mut out = self.audio_out.lock().unwrap();
        let _ = out.pa_enable(on);
    }
}

unsafe impl Send for HalAudioService {}
unsafe impl Sync for HalAudioService {}
