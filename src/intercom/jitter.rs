//! Jitter buffer + multi-source mixer (change 11/17). §3.5, §13.
#![allow(dead_code)]
use std::fmt;

use crate::services::audio_service::{AudioFrame, PCM_SAMPLES_PER_FRAME};

pub trait JitterBuffer: Send + Sync + fmt::Debug {
    fn push(&self, seq: u16, frame: AudioFrame);
    fn pop_ready(&self) -> Option<AudioFrame>;
    fn water_level(&self) -> u8;
}

pub trait Mixer: Send + Sync + fmt::Debug {
    fn submit(&self, src_id: u16, pcm: &[i16; PCM_SAMPLES_PER_FRAME]);
    fn mix(&self) -> [i16; PCM_SAMPLES_PER_FRAME];
}

pub struct JitterBufferStub;
pub struct MixerStub;

impl fmt::Debug for JitterBufferStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("JitterBufferStub").finish() }
}
impl fmt::Debug for MixerStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("MixerStub").finish() }
}

impl JitterBuffer for JitterBufferStub {
    fn push(&self, _: u16, _: AudioFrame) {}
    fn pop_ready(&self) -> Option<AudioFrame> { None }
    fn water_level(&self) -> u8 { 0 }
}
impl Mixer for MixerStub {
    fn submit(&self, _: u16, _: &[i16; PCM_SAMPLES_PER_FRAME]) {}
    fn mix(&self) -> [i16; PCM_SAMPLES_PER_FRAME] { [0i16; PCM_SAMPLES_PER_FRAME] }
}
unsafe impl Send for JitterBufferStub {}
unsafe impl Sync for JitterBufferStub {}
unsafe impl Send for MixerStub {}
unsafe impl Sync for MixerStub {}
