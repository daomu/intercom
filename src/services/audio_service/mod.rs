//! Audio service: Opus encode/decode + I2S + PA soft-start. change 05/17.
//! Spec: §3.4. design D1-D6 (see change 05 design.md).

#![allow(dead_code)]
#![allow(unused_imports)]

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

/// Audio service trait. §3.4.
pub trait AudioService: Send + Sync + fmt::Debug {
    fn start_capture(&self) -> Result<(), AudioError>;
    fn stop_capture(&self) -> Result<(), AudioError>;
    fn start_playback(&self) -> Result<(), AudioError>;
    fn stop_playback(&self) -> Result<(), AudioError>;
    /// Encode captured PCM (320 samples) → Opus frame.
    fn encode(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<AudioFrame, AudioError>;
    /// Decode Opus frame → 320 PCM samples.
    fn decode(&self, frame: &AudioFrame) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError>;
    /// Submit decoded PCM for playback (mixed with other streams).
    fn submit_pcm(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError>;
    /// PA soft-start: enable PA after 1-2 frames buffer; disable before stop.
    fn pa_enable(&self, on: bool);
}

/// Stub impl. Real Opus/I2S wiring deferred to on-hardware verification.
pub struct AudioServiceStub {
    _sample_rate: u32,
}

impl fmt::Debug for AudioServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioServiceStub").finish_non_exhaustive()
    }
}

impl AudioServiceStub {
    pub fn new() -> Self {
        Self {
            _sample_rate: BoardProfile::OPUS_SAMPLE_RATE,
        }
    }
}

impl Default for AudioServiceStub {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioService for AudioServiceStub {
    fn start_capture(&self) -> Result<(), AudioError> {
        Ok(())
    }
    fn stop_capture(&self) -> Result<(), AudioError> {
        Ok(())
    }
    fn start_playback(&self) -> Result<(), AudioError> {
        Ok(())
    }
    fn stop_playback(&self) -> Result<(), AudioError> {
        Ok(())
    }
    fn encode(&self, _pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<AudioFrame, AudioError> {
        Ok(AudioFrame {
            seq: 0,
            opus_data: [0u8; MAX_OPUS_FRAME_SIZE],
            opus_len: 0,
        })
    }
    fn decode(&self, _frame: &AudioFrame) -> Result<[i16; PCM_SAMPLES_PER_FRAME], AudioError> {
        Ok([0i16; PCM_SAMPLES_PER_FRAME])
    }
    fn submit_pcm(&self, _pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError> {
        Ok(())
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
}

impl fmt::Debug for HalAudioService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HalAudioService")
            .field("capturing", &self.capturing)
            .field("playing", &self.playing)
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
        }
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
        // Disable PA before stopping I2S to avoid pop.
        let mut out = self.audio_out.lock().unwrap();
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
    fn submit_pcm(&self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Result<(), AudioError> {
        let mut out = self.audio_out.lock().unwrap();
        // PA soft-start: if first frame since start_playback, enable PA now.
        if *self.playing.lock().unwrap() {
            let _ = out.pa_enable(true);
        }
        // Reinterpret [i16; N] as [u8; N*2] (little-endian, native I2S format).
        let bytes: &[u8] = unsafe {
            std::slice::from_raw_parts(pcm.as_ptr() as *const u8, std::mem::size_of_val(pcm))
        };
        out.write_pcm(&mut self.i2s.lock().unwrap(), bytes)
            .map_err(Self::map_err)?;
        Ok(())
    }
    fn pa_enable(&self, on: bool) {
        let mut out = self.audio_out.lock().unwrap();
        let _ = out.pa_enable(on);
    }
}

unsafe impl Send for HalAudioService {}
unsafe impl Sync for HalAudioService {}
