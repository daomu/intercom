//! Power + screen management policy (change 16/17). §3.6, §16, §9.
//! Extends change 04 PowerService with policy: 30s screen-off timer,
//! first-touch wake-only, low-battery standby, PWR short-press toggle.
#![allow(dead_code)]
use std::fmt;

use crate::board_profile::BoardProfile;

pub trait PowerManagementPolicy: Send + Sync + fmt::Debug {
    /// Called every 1s tick; returns Some(off) when screen-off timer expires.
    fn tick(&self) -> Option<ScreenOffReason>;
    /// User activity (touch/button); resets the screen-off timer.
    fn note_activity(&self);
    /// PWR short press → toggle screen on/off.
    fn on_power_short_press(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenOffReason {
    Timeout,
    LowBattery,
    UserRequest,
}

pub struct PowerManagementStub {
    timeout_sec: u32,
}

impl fmt::Debug for PowerManagementStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PowerManagementStub")
            .field("timeout_sec", &self.timeout_sec)
            .finish()
    }
}

impl PowerManagementStub {
    pub fn new() -> Self {
        Self { timeout_sec: BoardProfile::DEFAULT_SCREEN_OFF_SEC }
    }
}
impl Default for PowerManagementStub {
    fn default() -> Self { Self::new() }
}

impl PowerManagementPolicy for PowerManagementStub {
    fn tick(&self) -> Option<ScreenOffReason> { None }
    fn note_activity(&self) {}
    fn on_power_short_press(&self) -> bool { true }
}
unsafe impl Send for PowerManagementStub {}
unsafe impl Sync for PowerManagementStub {}
