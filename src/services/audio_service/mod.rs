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
