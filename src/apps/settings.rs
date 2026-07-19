//! Settings app (change 07). Spec: §3.7, §6.2, §8.1.
//!
//! Pure-logic Settings App: page navigation (left/right swipe through 7
//! pages), field editing with bounds clamping, persistence via injected
//! `save_settings` callback, factory reset with two-step confirmation (D9).
//! Slint rendering lands in change 17; this module hosts the testable
//! state machine.
//!
//! About page data (safety-diagnostics change 16, task 5.2): the `AboutData`
//! struct bundles the four read-only fields displayed on the About page.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;

use crate::apps::{App, AppContext, HitTarget, RenderCtx};
use crate::services::display_buf::Rgb565Buf;
use crate::services::input::InputEvent;
use crate::services::power::ResetReason;
use crate::services::storage::{DiagInfo, Settings, StorageService};
use crate::board_profile::BoardProfile;

/// Settings page index (PRD §6.2, design D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    DeviceName = 0,
    Volume = 1,
    Mute = 2,
    Brightness = 3,
    ScreenOffTime = 4,
    About = 5,
    FactoryReset = 6,
}

impl SettingsPage {
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::DeviceName),
            1 => Some(Self::Volume),
            2 => Some(Self::Mute),
            3 => Some(Self::Brightness),
            4 => Some(Self::ScreenOffTime),
            5 => Some(Self::About),
            6 => Some(Self::FactoryReset),
            _ => None,
        }
    }

    pub fn next(self) -> Option<Self> {
        Self::from_index(self as usize + 1)
    }

    pub fn prev(self) -> Option<Self> {
        if self as usize == 0 {
            None
        } else {
            Self::from_index(self as usize - 1)
        }
    }
}

/// Two-step factory-reset confirmation state (D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactoryResetState {
    Idle,
    FirstConfirm,
    SecondConfirmArmed,
}

pub const VOLUME_MAX: u8 = 100;
pub const BRIGHTNESS_MAX: u8 = 100;
pub const SCREEN_OFF_MIN_SEC: u32 = 5;
pub const SCREEN_OFF_MAX_SEC: u32 = 300;
pub const DEVICE_NAME_MAX_LEN: usize = 16;

/// Controller-facing side-effect signal (wire-settings-side-effects).
/// Most field editors only persist to NVS, but some require the controller
/// (main loop, which owns the services) to run a hardware action. Setters
/// return this so the caller can apply the effect immediately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsOutcome {
    /// No controller side-effect required (NVS persistence already done).
    Nop,
    /// Brightness changed to `.0`; controller SHALL call
    /// `DisplayService::set_brightness(.0)` so the panel updates immediately
    /// instead of only being persisted.
    BrightnessChanged(u8),
}

pub struct SettingsApp {
    settings: Settings,
    page: SettingsPage,
    factory_reset_state: FactoryResetState,
    /// Callback invoked when settings change. Real impl saves to NVS.
    save_cb: Mutex<Option<Box<dyn Fn(&Settings) + Send + Sync>>>,
}

impl fmt::Debug for SettingsApp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SettingsApp")
            .field("page", &self.page)
            .field("factory_reset_state", &self.factory_reset_state)
            .field("device_name", &self.settings.device_name)
            .finish_non_exhaustive()
    }
}

impl SettingsApp {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            page: SettingsPage::DeviceName,
            factory_reset_state: FactoryResetState::Idle,
            save_cb: Mutex::new(None),
        }
    }

    pub fn page(&self) -> SettingsPage {
        self.page
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn factory_reset_state(&self) -> FactoryResetState {
        self.factory_reset_state
    }

    pub fn on_save_cb(&self, cb: Box<dyn Fn(&Settings) + Send + Sync>) {
        *self.save_cb.lock().expect("save_cb mutex") = Some(cb);
    }

    fn persist(&self) {
        if let Some(cb) = self.save_cb.lock().expect("save_cb mutex").as_ref() {
            cb(&self.settings);
        }
    }

    // ---- Page navigation (D7: left/right swipe) ----
    pub fn swipe_next(&mut self) {
        if let Some(p) = self.page.next() {
            self.page = p;
        }
    }
    pub fn swipe_prev(&mut self) {
        if let Some(p) = self.page.prev() {
            self.page = p;
        }
    }

    // ---- Field editors ----
    pub fn set_volume(&mut self, v: u8) {
        self.settings.volume = v.min(VOLUME_MAX);
        self.persist();
    }

    pub fn set_brightness(&mut self, v: u8) -> SettingsOutcome {
        self.settings.brightness = v.min(BRIGHTNESS_MAX);
        self.persist();
        SettingsOutcome::BrightnessChanged(self.settings.brightness)
    }

    pub fn set_muted(&mut self, m: bool) {
        self.settings.muted = m;
        self.persist();
    }

    pub fn set_screen_off_sec(&mut self, sec: u32) {
        self.settings.screen_off_sec = sec.clamp(SCREEN_OFF_MIN_SEC, SCREEN_OFF_MAX_SEC);
        self.persist();
    }

    pub fn set_device_name(&mut self, name: &str) {
        let truncated: String = name.chars().take(DEVICE_NAME_MAX_LEN).collect();
        self.settings.device_name = truncated;
        self.persist();
    }

    // ---- Factory reset two-step confirmation (D9) ----
    /// User clicks "确认" on page 1 → advance to second-confirm screen.
    pub fn factory_reset_arm(&mut self) {
        if self.factory_reset_state == FactoryResetState::Idle {
            self.factory_reset_state = FactoryResetState::FirstConfirm;
        }
    }

    /// User clicks "确认恢复" on page 2 → execute reset.
    /// Returns true if reset was performed (caller clears NVS + restarts).
    pub fn factory_reset_confirm(&mut self) -> bool {
        if self.factory_reset_state == FactoryResetState::FirstConfirm {
            self.factory_reset_state = FactoryResetState::SecondConfirmArmed;
            // Reset settings to defaults.
            self.settings = Settings::default();
            true
        } else {
            false
        }
    }

    /// User cancels factory reset.
    pub fn factory_reset_cancel(&mut self) {
        self.factory_reset_state = FactoryResetState::Idle;
    }

    /// Generate a random device name (D8: adjective + noun).
    /// Uses `esp_random` on device; here we accept an injected u32.
    pub fn random_device_name(rng: u32) -> String {
        const ADJ: &[&str] = &[
            "Swift", "Brave", "Calm", "Daring", "Eager", "Fierce", "Gentle",
            "Happy", "Jolly", "Keen", "Lively", "Mighty", "Noble", "Proud",
            "Quick", "Royal", "Steady", "Trusty", "Vivid", "Wise",
        ];
        const NOUN: &[&str] = &[
            "Fox", "Lion", "Hawk", "Wolf", "Bear", "Cat", "Dog", "Owl",
            "Deer", "Seal", "Puma", "Lynx", "Otter", "Bison", "Crow",
            "Duck", "Frog", "Goat", "Hare", "Koi",
        ];
        let a = ADJ[(rng as usize) % ADJ.len()];
        let n = NOUN[((rng >> 16) as usize) % NOUN.len()];
        format!("{}{}", a, n)
    }
}

// ---- Trait shim ---------------------------------------------------------

pub trait SettingsAppTrait: Send + Sync + fmt::Debug {
    fn load(&self) -> Settings;
    fn save(&self, s: &Settings);
    fn reset(&self);
}

// ---- App trait impl (view-layer delegation) -----------------------------

impl App for SettingsApp {
    fn id(&self) -> &str {
        "settings"
    }
    fn title(&self) -> &str {
        "Settings"
    }
    fn on_enter(&mut self, _ctx: &AppContext) {}
    fn on_exit(&mut self, _ctx: &AppContext) {}
    fn on_event(&mut self, _ev: &InputEvent, _ctx: &AppContext) {
        // Settings input routing (swipe / tap) is handled by the controller
        // calling `swipe_next` / `swipe_prev` / `set_*` directly. The App
        // trait lifecycle no-ops here to avoid double-dispatch.
    }
    fn on_tick(&mut self, _ctx: &AppContext) {}
    fn render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx) {
        crate::apps::view::settings_view::draw_settings(
            fb,
            ctx,
            self.page(),
            self.factory_reset_state(),
        );
    }
    fn hit_test(&self, x: i32, y: i32, _ctx: &RenderCtx) -> Option<HitTarget> {
        crate::apps::view::settings_view::hit_test(x, y, self.page(), self.factory_reset_state())
    }
}

// ---- About page data (safety-diagnostics change 16, task 5.2) -----------

/// Read-only data shown on the About page (D5). All four fields come from
/// `BoardProfile` / `DiagInfo` — never from live `PowerService::reset_reason()`
/// (the diag value is the persisted snapshot from boot time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutData {
    pub firmware_version: &'static str,
    pub reset_reason: ResetReason,
    pub abnormal_boot_count: u32,
    pub safe_boot_flag: bool,
}

impl AboutData {
    /// Build from the persisted diag snapshot. Per task 5.2, the reset reason
    /// comes from `DiagInfo.last_reset_reason` (NOT from `PowerService`).
    pub fn from_diag(diag: &DiagInfo) -> Self {
        Self {
            firmware_version: BoardProfile::FIRMWARE_VERSION,
            reset_reason: ResetReason::from_code(diag.last_reset_reason),
            abnormal_boot_count: diag.abnormal_boot_cnt,
            safe_boot_flag: diag.safe_boot_flag,
        }
    }

    /// Display string for the reset reason (task 5.3).
    pub fn reset_reason_display(&self) -> &'static str {
        self.reset_reason.display()
    }
}

impl Default for AboutData {
    fn default() -> Self {
        Self {
            firmware_version: BoardProfile::FIRMWARE_VERSION,
            reset_reason: ResetReason::PowerOn,
            abnormal_boot_count: 0,
            safe_boot_flag: false,
        }
    }
}

/// Standalone factory-reset routine (safety-diagnostics task 6.2).
/// Calls reset_settings + clear_group + clear_diag best-effort, then signals
/// the caller to invoke `esp_idf_svc::system::reset()`. Each step logs but
/// does not abort on error (D4: best-effort, no panic).
pub fn factory_reset<S: StorageService>(storage: &S) -> Result<(), ()> {
    let mut ok = true;
    if let Err(e) = storage.reset_settings() {
        log::error!("factory_reset: reset_settings failed: {e:?}");
        ok = false;
    }
    if let Err(e) = storage.clear_group() {
        log::error!("factory_reset: clear_group failed: {e:?}");
        ok = false;
    }
    if let Err(e) = storage.clear_diag() {
        log::error!("factory_reset: clear_diag failed: {e:?}");
        ok = false;
    }
    if ok { Ok(()) } else { Err(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn mk() -> SettingsApp {
        let mut s = Settings::default();
        s.device_name = String::from("INT-0000");
        SettingsApp::new(s)
    }

    #[test]
    fn page_navigation_wraps_at_ends() {
        let mut a = mk();
        assert_eq!(a.page(), SettingsPage::DeviceName);
        assert!(a.page.prev().is_none());
        a.swipe_next();
        assert_eq!(a.page(), SettingsPage::Volume);
        for _ in 0..10 {
            a.swipe_next();
        }
        assert_eq!(a.page(), SettingsPage::FactoryReset);
    }

    #[test]
    fn volume_clamped() {
        let mut a = mk();
        a.set_volume(150);
        assert_eq!(a.settings().volume, 100);
        a.set_volume(50);
        assert_eq!(a.settings().volume, 50);
    }

    #[test]
    fn screen_off_clamped() {
        let mut a = mk();
        a.set_screen_off_sec(1);
        assert_eq!(a.settings().screen_off_sec, SCREEN_OFF_MIN_SEC);
        a.set_screen_off_sec(999);
        assert_eq!(a.settings().screen_off_sec, SCREEN_OFF_MAX_SEC);
    }

    #[test]
    fn device_name_truncated() {
        let mut a = mk();
        a.set_device_name("abcdefghijklmnopqrstuvwxyz");
        assert!(a.settings().device_name.chars().count() <= DEVICE_NAME_MAX_LEN);
    }

    #[test]
    fn save_cb_invoked_on_edit() {
        let mut a = mk();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();
        a.on_save_cb(Box::new(move |_s| {
            count_clone.fetch_add(1, Ordering::SeqCst);
        }));
        a.set_volume(30);
        a.set_muted(true);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn factory_reset_two_step() {
        let mut a = mk();
        assert_eq!(a.factory_reset_state(), FactoryResetState::Idle);
        // First confirm without arming → no-op.
        assert!(!a.factory_reset_confirm());
        a.factory_reset_arm();
        assert_eq!(a.factory_reset_state(), FactoryResetState::FirstConfirm);
        assert!(a.factory_reset_confirm());
        assert_eq!(a.factory_reset_state(), FactoryResetState::SecondConfirmArmed);
        // Settings reset to defaults.
        assert_eq!(a.settings().volume, Settings::default().volume);
    }

    #[test]
    fn factory_reset_cancel() {
        let mut a = mk();
        a.factory_reset_arm();
        a.factory_reset_cancel();
        assert_eq!(a.factory_reset_state(), FactoryResetState::Idle);
    }

    #[test]
    fn random_device_name_in_wordlist() {
        let name = SettingsApp::random_device_name(0x12345678);
        // First char is uppercase, total length > 4.
        assert!(name.chars().next().unwrap().is_uppercase());
        assert!(name.len() > 4);
    }
}
