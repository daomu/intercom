//! Voice PTT state machine (change 10/17). §3.5, §11.
#![allow(dead_code)]
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PttState {
    Idle,
    PttArming,
    PttActive,
    Listening,
}

pub trait VoicePttService: Send + Sync + fmt::Debug {
    fn on_boot_press(&self, screen_was_off: bool);
    fn on_boot_release(&self);
    fn current_state(&self) -> PttState;
}

pub struct VoicePttServiceStub;
impl fmt::Debug for VoicePttServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoicePttServiceStub").finish()
    }
}
impl VoicePttService for VoicePttServiceStub {
    fn on_boot_press(&self, _: bool) {}
    fn on_boot_release(&self) {}
    fn current_state(&self) -> PttState { PttState::Idle }
}
unsafe impl Send for VoicePttServiceStub {}
unsafe impl Sync for VoicePttServiceStub {}
