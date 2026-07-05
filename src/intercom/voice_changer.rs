//! Voice changer (change 14/17). §3.5 voice transform.
#![allow(dead_code)]
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoicePreset {
    Original,
    Deep,
    Robot,
    Kid,
}

pub trait VoiceChanger: Send + Sync + fmt::Debug {
    fn set_preset(&self, preset: VoicePreset);
    fn process(&self, pcm: &mut [i16]);
}

pub struct VoiceChangerStub;
impl fmt::Debug for VoiceChangerStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("VoiceChangerStub").finish() }
}
impl VoiceChanger for VoiceChangerStub {
    fn set_preset(&self, _: VoicePreset) {}
    fn process(&self, _: &mut [i16]) {}
}
unsafe impl Send for VoiceChangerStub {}
unsafe impl Sync for VoiceChangerStub {}
