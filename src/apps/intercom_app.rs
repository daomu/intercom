//! Intercom app UI (change 13/17). §3.7 main intercom interface.
#![allow(dead_code)]
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntercomUiState {
    Idle,
    Listening,
    PttArming,
    PttActive,
}

pub trait IntercomApp: Send + Sync + fmt::Debug {
    fn current_state(&self) -> IntercomUiState;
    fn on_ptt_press(&self);
    fn on_ptt_release(&self);
    fn on_incoming_voice(&self, src_id: u16);
}

pub struct IntercomAppStub;
impl fmt::Debug for IntercomAppStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("IntercomAppStub").finish() }
}
impl IntercomApp for IntercomAppStub {
    fn current_state(&self) -> IntercomUiState { IntercomUiState::Idle }
    fn on_ptt_press(&self) {}
    fn on_ptt_release(&self) {}
    fn on_incoming_voice(&self, _: u16) {}
}
unsafe impl Send for IntercomAppStub {}
unsafe impl Sync for IntercomAppStub {}
