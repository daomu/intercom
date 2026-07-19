//! Voice codec: Opus (feature-gated) with a PCM passthrough fallback.
//! Spec: §11 (Opus). change: wire-audio-pipeline (tasks 3.1-3.4).
//!
//! The `opus` cargo feature is OFF by default (esp-idf cross toolchain may
//! lack libopus). Without it, `PcmPassThrough` byte-casts 16-bit LE PCM
//! directly (640 bytes / 20ms frame) so the I2S + jitter + mixing pipeline
//! can be verified end-to-end. When the feature is enabled and the FFI is
//! wired, `IntercomCodec::Opus` takes over transparently.

#![allow(dead_code)]

use crate::services::audio_service::PCM_SAMPLES_PER_FRAME;

/// Voice codec used by the audio thread.
pub enum IntercomCodec {
    /// 16-bit LE PCM byte-cast (no compression). 640 bytes / 20ms frame.
    PcmPassThrough,
    /// Opus 16kHz mono ~32kbps (feature-gated; FFI wiring deferred).
    #[cfg(feature = "opus")]
    Opus,
}

impl IntercomCodec {
    /// Factory: `opus_enabled` selects the Opus path when the feature is
    /// compiled in; otherwise falls back to PCM passthrough (task 3.4).
    pub fn new(opus_enabled: bool) -> Self {
        #[cfg(feature = "opus")]
        {
            if opus_enabled {
                // TODO(task 3.3): construct opus::Encoder/Decoder (16kHz mono,
                // 20ms frame, 32kbps). Until the FFI is wired, fall through to
                // PCM passthrough so the pipeline still runs.
                return IntercomCodec::PcmPassThrough;
            }
        }
        let _ = opus_enabled;
        IntercomCodec::PcmPassThrough
    }

    /// True when the active path is Opus (vs PCM passthrough).
    pub fn is_opus(&self) -> bool {
        match self {
            IntercomCodec::PcmPassThrough => false,
            #[cfg(feature = "opus")]
            IntercomCodec::Opus => true,
        }
    }

    /// Encode one captured PCM frame → wire payload (task 3.2).
    pub fn encode(&mut self, pcm: &[i16; PCM_SAMPLES_PER_FRAME]) -> Vec<u8> {
        match self {
            IntercomCodec::PcmPassThrough => {
                let mut out = Vec::with_capacity(PCM_SAMPLES_PER_FRAME * 2);
                for &s in pcm.iter() {
                    out.extend_from_slice(&s.to_le_bytes());
                }
                out
            }
            #[cfg(feature = "opus")]
            IntercomCodec::Opus => {
                // TODO(task 3.3): opus encode.
                Vec::new()
            }
        }
    }

    /// Decode a wire payload → one PCM frame, zero-padded / truncated to
    /// `PCM_SAMPLES_PER_FRAME` (task 3.2).
    pub fn decode(&mut self, payload: &[u8]) -> [i16; PCM_SAMPLES_PER_FRAME] {
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        match self {
            IntercomCodec::PcmPassThrough => {
                let n = (payload.len() / 2).min(PCM_SAMPLES_PER_FRAME);
                for (i, s) in pcm.iter_mut().enumerate().take(n) {
                    *s = i16::from_le_bytes([payload[i * 2], payload[i * 2 + 1]]);
                }
            }
            #[cfg(feature = "opus")]
            IntercomCodec::Opus => {
                // TODO(task 3.3): opus decode.
            }
        }
        pcm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_passthrough_round_trip() {
        let mut codec = IntercomCodec::new(false);
        assert!(!codec.is_opus());
        let mut pcm = [0i16; PCM_SAMPLES_PER_FRAME];
        for (i, s) in pcm.iter_mut().enumerate() {
            *s = (i as i16).wrapping_mul(37).wrapping_sub(12000);
        }
        let payload = codec.encode(&pcm);
        assert_eq!(payload.len(), PCM_SAMPLES_PER_FRAME * 2);
        let back = codec.decode(&payload);
        assert_eq!(back, pcm);
    }

    #[test]
    fn decode_short_payload_zero_pads() {
        let mut codec = IntercomCodec::new(false);
        // Only 2 samples worth of bytes; the rest must be silence.
        let payload = [0x10, 0x20, 0x30, 0x40];
        let pcm = codec.decode(&payload);
        assert_eq!(pcm[0], i16::from_le_bytes([0x10, 0x20]));
        assert_eq!(pcm[1], i16::from_le_bytes([0x30, 0x40]));
        assert_eq!(pcm[2], 0);
        assert_eq!(pcm[PCM_SAMPLES_PER_FRAME - 1], 0);
    }
}
