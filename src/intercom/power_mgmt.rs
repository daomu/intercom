//! Power + screen management policy (change 15). Spec: §3.6, §16, §9.
//!
//! Pure-logic policy layer coordinating DisplayService / InputService /
//! PowerService / AudioService:
//! - `ScreenPolicy`: 30s screen-off timer (D2), wake-first-touch filter
//!   (D3), wake sources (touch / PWR short / BOOT short), screen-off PTT
//!   bypass (D4).
//! - `StandbyPolicy`: 4-condition AND gate for non-interop standby (D5).
//! - `PaController`: thin wrapper around AudioService start/stop (D6).
//! - `LowPowerProtection`: trait skeleton, unimplemented (D8).

#![allow(dead_code)]

use std::fmt;
use std::time::Duration;

use crate::board_profile::BoardProfile;
use crate::intercom::state::{IntercomState, VoiceState};

// ---- Constants (PRD §9, §26.6) -------------------------------------------

pub const DEFAULT_SCREEN_OFF_SEC: u32 = BoardProfile::DEFAULT_SCREEN_OFF_SEC;
pub const STANDBY_GRACE_SEC: u32 = 60;

// ---- ScreenPolicy (D2, D3, D4) ------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenState {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenOffReason {
    Timeout,
    LowBattery,
    UserRequest,
}

/// Input events the ScreenPolicy consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenInputEvent {
    /// Any touch down.
    TouchDown,
    /// Any touch up.
    TouchUp,
    /// BOOT button short tap (< 50ms, screen-wake gesture).
    BootShortTap,
    /// BOOT button long press (≥ 50ms, PTT gesture — bypasses screen policy).
    BootPress,
    /// PWR button short press (toggle screen on/off).
    PowerShortPress,
}

/// Output actions the ScreenPolicy requests the caller to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenAction {
    /// No action.
    None,
    /// Turn the screen on (and reset the activity timer).
    ScreenOn,
    /// Turn the screen off (and notify StandbyPolicy).
    ScreenOff(ScreenOffReason),
    /// Forward the input event to UI / Intercom layer.
    ForwardEvent(ScreenInputEvent),
    /// Drop the input event (first wake-touch consumed per D3).
    ConsumeEvent,
    /// Forward to IntercomService as PTT press (D4: screen-off PTT bypass).
    ForwardPttPress { screen_was_off: bool },
}

pub struct ScreenPolicy {
    state: ScreenState,
    /// Monotonic ms of last user activity.
    last_activity_ms: u64,
    /// Whether the first post-wake touch has been consumed (D3).
    first_touch_consumed: bool,
    /// Configured screen-off timeout (seconds).
    screen_off_sec: u32,
}

impl fmt::Debug for ScreenPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScreenPolicy")
            .field("state", &self.state)
            .field("first_touch_consumed", &self.first_touch_consumed)
            .field("screen_off_sec", &self.screen_off_sec)
            .finish_non_exhaustive()
    }
}

impl ScreenPolicy {
    pub fn new(screen_off_sec: u32) -> Self {
        Self {
            state: ScreenState::On,
            last_activity_ms: 0,
            first_touch_consumed: false,
            screen_off_sec,
        }
    }

    pub fn state(&self) -> ScreenState {
        self.state
    }

    pub fn is_screen_on(&self) -> bool {
        self.state == ScreenState::On
    }

    /// Called by Task C every 1s.
    pub fn tick(&mut self, now_ms: u64) -> Option<ScreenOffReason> {
        if self.state != ScreenState::On {
            return None;
        }
        let elapsed = now_ms - self.last_activity_ms;
        if elapsed >= (self.screen_off_sec as u64) * 1000 {
            self.state = ScreenState::Off;
            self.first_touch_consumed = false;
            return Some(ScreenOffReason::Timeout);
        }
        None
    }

    /// Mark any user activity (resets timer).
    pub fn note_activity(&mut self, now_ms: u64) {
        self.last_activity_ms = now_ms;
    }

    /// Force the screen off (e.g. low battery).
    pub fn force_off(&mut self, reason: ScreenOffReason, _now_ms: u64) -> ScreenAction {
        if self.state == ScreenState::On {
            self.state = ScreenState::Off;
            self.first_touch_consumed = false;
            return ScreenAction::ScreenOff(reason);
        }
        ScreenAction::None
    }

    /// Process an input event. Returns the action to take.
    pub fn on_event(&mut self, event: ScreenInputEvent, now_ms: u64) -> ScreenAction {
        use ScreenInputEvent::*;
        match (self.state, event) {
            // ---- Screen ON: forward all events, reset timer on activity ----
            (ScreenState::On, TouchDown) => {
                self.note_activity(now_ms);
                ScreenAction::ForwardEvent(TouchDown)
            }
            (ScreenState::On, TouchUp) => {
                self.note_activity(now_ms);
                ScreenAction::ForwardEvent(TouchUp)
            }
            (ScreenState::On, BootShortTap) => {
                self.note_activity(now_ms);
                ScreenAction::ForwardEvent(BootShortTap)
            }
            (ScreenState::On, BootPress) => {
                self.note_activity(now_ms);
                ScreenAction::ForwardPttPress { screen_was_off: false }
            }
            (ScreenState::On, PowerShortPress) => {
                self.note_activity(now_ms);
                self.state = ScreenState::Off;
                self.first_touch_consumed = false;
                ScreenAction::ScreenOff(ScreenOffReason::UserRequest)
            }

            // ---- Screen OFF: wake sources ----
            (ScreenState::Off, TouchDown) => {
                // D3: first touch wakes the screen but is consumed.
                self.state = ScreenState::On;
                self.first_touch_consumed = true;
                self.note_activity(now_ms);
                ScreenAction::ScreenOn
            }
            (ScreenState::Off, BootShortTap) => {
                self.state = ScreenState::On;
                self.first_touch_consumed = true;
                self.note_activity(now_ms);
                ScreenAction::ScreenOn
            }
            (ScreenState::Off, PowerShortPress) => {
                self.state = ScreenState::On;
                self.first_touch_consumed = true;
                self.note_activity(now_ms);
                ScreenAction::ScreenOn
            }
            (ScreenState::Off, BootPress) => {
                // D4: screen-off PTT bypass — don't wake the screen.
                // Don't reset the activity timer either (per D45 cross-ref).
                ScreenAction::ForwardPttPress { screen_was_off: true }
            }
            (ScreenState::Off, TouchUp) => {
                // Touch up while screen off — consume silently.
                ScreenAction::ConsumeEvent
            }
        }
    }

    /// After the wake-touch cycle, the next TouchDown is forwarded normally
    /// once first_touch_consumed is cleared. We clear it on the first TouchUp
    /// after wake.
    pub fn clear_first_touch(&mut self) {
        self.first_touch_consumed = false;
    }
}

// ---- StandbyPolicy (D5) --------------------------------------------------

/// Conditions that must ALL be true to enter non-interop standby.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StandbyConditions {
    /// IntercomState == Idle or Grouped(Idle).
    pub system_idle: bool,
    /// No foreground persistent task (no pairing in progress, etc.).
    pub no_foreground_task: bool,
    /// No user interaction for STANDBY_GRACE_SEC.
    pub no_user_interaction: bool,
    /// AudioService capture + playback both stopped.
    pub audio_idle: bool,
}

impl StandbyConditions {
    pub fn all_true(&self) -> bool {
        self.system_idle && self.no_foreground_task && self.no_user_interaction && self.audio_idle
    }
}

pub struct StandbyPolicy {
    last_user_interaction_ms: u64,
    in_standby: bool,
}

impl fmt::Debug for StandbyPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StandbyPolicy")
            .field("in_standby", &self.in_standby)
            .finish_non_exhaustive()
    }
}

impl StandbyPolicy {
    pub fn new() -> Self {
        Self {
            last_user_interaction_ms: 0,
            in_standby: false,
        }
    }

    pub fn in_standby(&self) -> bool {
        self.in_standby
    }

    pub fn note_user_interaction(&mut self, now_ms: u64) {
        self.last_user_interaction_ms = now_ms;
        if self.in_standby {
            self.in_standby = false;
        }
    }

    /// Evaluate whether to enter/leave standby. Returns the requested action.
    pub fn evaluate(
        &mut self,
        now_ms: u64,
        state: &IntercomState,
        no_foreground_task: bool,
        audio_idle: bool,
    ) -> StandbyAction {
        let system_idle = matches!(
            state,
            IntercomState::Idle | IntercomState::Grouped(VoiceState::Idle)
        );
        let no_interaction = now_ms - self.last_user_interaction_ms
            >= (STANDBY_GRACE_SEC as u64) * 1000;
        let cond = StandbyConditions {
            system_idle,
            no_foreground_task,
            no_user_interaction: no_interaction,
            audio_idle,
        };
        if cond.all_true() && !self.in_standby {
            self.in_standby = true;
            StandbyAction::EnterStandby
        } else if !cond.all_true() && self.in_standby {
            self.in_standby = false;
            StandbyAction::LeaveStandby
        } else {
            StandbyAction::None
        }
    }
}

impl Default for StandbyPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandbyAction {
    None,
    EnterStandby,
    LeaveStandby,
}

// ---- PaController (D6) ---------------------------------------------------

/// PA control is delegated to AudioService start/stop_playback (D6).
/// This struct is a thin marker — callers invoke AudioService methods
/// directly; PaController exists only as a documentation anchor.
pub struct PaController;

impl PaController {
    /// Called before entering Listening / Talking.
    pub const fn on_voice_active() -> &'static str {
        "AudioService::start_playback"
    }
    /// Called after exiting Listening / Talking.
    pub const fn on_voice_idle() -> &'static str {
        "AudioService::stop_playback"
    }
}

// ---- LowPowerProtection trait (D8) --------------------------------------

/// Low-power protection trait — interface reservation only (D8). Not
/// implemented in phase 1.
pub trait LowPowerProtection: Send + Sync + fmt::Debug {
    /// Check conditions and act (e.g. auto-derate, force screen-off, low-battery alert).
    /// Returns a description of the action taken, if any.
    fn check_and_act(&self) -> Option<&'static str>;
}

pub struct LowPowerProtectionStub;

impl fmt::Debug for LowPowerProtectionStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LowPowerProtectionStub").finish()
    }
}

impl LowPowerProtection for LowPowerProtectionStub {
    fn check_and_act(&self) -> Option<&'static str> {
        None
    }
}
unsafe impl Send for LowPowerProtectionStub {}
unsafe impl Sync for LowPowerProtectionStub {}

// ---- Trait shim ---------------------------------------------------------

pub trait PowerManagementPolicy: Send + Sync + fmt::Debug {
    fn tick(&self) -> Option<ScreenOffReason>;
    fn note_activity(&self);
    fn on_power_short_press(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_timeout_after_30s() {
        let mut p = ScreenPolicy::new(30);
        p.note_activity(0);
        assert!(p.tick(29_000).is_none());
        assert_eq!(p.tick(30_000), Some(ScreenOffReason::Timeout));
        assert_eq!(p.state(), ScreenState::Off);
    }

    #[test]
    fn wake_touch_consumed() {
        let mut p = ScreenPolicy::new(30);
        p.note_activity(0);
        p.tick(30_000); // screen off
        // First touch wakes screen, event is consumed.
        let a = p.on_event(ScreenInputEvent::TouchDown, 31_000);
        assert!(matches!(a, ScreenAction::ScreenOn));
        assert_eq!(p.state(), ScreenState::On);
        // Next touch forwarded.
        let a = p.on_event(ScreenInputEvent::TouchDown, 32_000);
        assert!(matches!(a, ScreenAction::ForwardEvent(_)));
    }

    #[test]
    fn screen_off_ptt_does_not_wake() {
        let mut p = ScreenPolicy::new(30);
        p.note_activity(0);
        p.tick(30_000);
        let a = p.on_event(ScreenInputEvent::BootPress, 31_000);
        assert!(matches!(
            a,
            ScreenAction::ForwardPttPress { screen_was_off: true }
        ));
        assert_eq!(p.state(), ScreenState::Off);
    }

    #[test]
    fn screen_on_ptt_forwards() {
        let mut p = ScreenPolicy::new(30);
        p.note_activity(0);
        let a = p.on_event(ScreenInputEvent::BootPress, 1_000);
        assert!(matches!(
            a,
            ScreenAction::ForwardPttPress { screen_was_off: false }
        ));
    }

    #[test]
    fn power_short_toggles_screen() {
        let mut p = ScreenPolicy::new(30);
        p.note_activity(0);
        // On → Off
        let a = p.on_event(ScreenInputEvent::PowerShortPress, 1_000);
        assert!(matches!(a, ScreenAction::ScreenOff(ScreenOffReason::UserRequest)));
        // Off → On
        let a = p.on_event(ScreenInputEvent::PowerShortPress, 2_000);
        assert!(matches!(a, ScreenAction::ScreenOn));
    }

    #[test]
    fn standby_requires_all_conditions() {
        let mut s = StandbyPolicy::new();
        s.note_user_interaction(0);
        // All conditions true → enter standby.
        let a = s.evaluate(
            61_000,
            &IntercomState::Grouped(VoiceState::Idle),
            true,
            true,
        );
        assert_eq!(a, StandbyAction::EnterStandby);
        // Audio no longer idle → leave standby.
        let a = s.evaluate(
            62_000,
            &IntercomState::Grouped(VoiceState::Idle),
            true,
            false,
        );
        assert_eq!(a, StandbyAction::LeaveStandby);
    }

    #[test]
    fn standby_blocked_by_talking_state() {
        let mut s = StandbyPolicy::new();
        s.note_user_interaction(0);
        let a = s.evaluate(
            61_000,
            &IntercomState::Grouped(VoiceState::Talking),
            true,
            true,
        );
        assert_eq!(a, StandbyAction::None);
    }

    #[test]
    fn standby_blocked_by_recent_interaction() {
        let mut s = StandbyPolicy::new();
        s.note_user_interaction(0);
        let a = s.evaluate(
            30_000, // < 60s grace
            &IntercomState::Grouped(VoiceState::Idle),
            true,
            true,
        );
        assert_eq!(a, StandbyAction::None);
    }

    #[test]
    fn user_interaction_leaves_standby() {
        let mut s = StandbyPolicy::new();
        s.note_user_interaction(0);
        s.evaluate(61_000, &IntercomState::Grouped(VoiceState::Idle), true, true);
        assert!(s.in_standby());
        s.note_user_interaction(62_000);
        assert!(!s.in_standby());
    }
}
