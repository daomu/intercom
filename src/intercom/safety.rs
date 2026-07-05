//! Safety + diagnostics (change 15/17). §3.5, §15.
#![allow(dead_code)]
use std::fmt;

use crate::services::power::ResetReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyAction {
    None,
    Warn,
    SafeBoot,
    ForceStandby,
}

pub trait SafetyService: Send + Sync + fmt::Debug {
    fn evaluate(&self, reset: ResetReason, abnormal_boot_cnt: u32) -> SafetyAction;
    fn safe_boot_flag(&self) -> bool;
    fn set_safe_boot_flag(&self, v: bool);
}

pub struct SafetyServiceStub;
impl fmt::Debug for SafetyServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("SafetyServiceStub").finish() }
}
impl SafetyService for SafetyServiceStub {
    fn evaluate(&self, _r: ResetReason, _c: u32) -> SafetyAction { SafetyAction::None }
    fn safe_boot_flag(&self) -> bool { false }
    fn set_safe_boot_flag(&self, _: bool) {}
}
unsafe impl Send for SafetyServiceStub {}
unsafe impl Sync for SafetyServiceStub {}
