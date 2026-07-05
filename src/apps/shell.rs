//! App shell / launcher (change 07). Spec: §3.7, §8.1, §15, §16, §24.
//!
//! Pure-logic launcher: App trait, AppRegistry, foreground/overlay state,
//! input dispatch priority (overlay > foreground > global shortcuts),
//! screen-off timer. Real slint Window wiring lands in change 17
//! integration; this module hosts the testable state machine.

#![allow(dead_code)]

use std::fmt;

use crate::services::input::InputEvent;
use crate::services::storage::Settings;

// ---- AppId + App trait --------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppId {
    Launcher,
    Settings,
    Intercom,
    About,
}

impl AppId {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppId::Launcher => "launcher",
            AppId::Settings => "settings",
            AppId::Intercom => "intercom",
            AppId::About => "about",
        }
    }
}

/// Page-stack overlay (volume panel / mute indicator / etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    VolumePanel,
    MuteToggleFlash,
}

/// Pure-logic launcher state machine.
pub struct Launcher {
    foreground: AppId,
    overlay: Option<Overlay>,
    /// Visible apps (PRD §24: only expose truly available apps).
    visible_apps: Vec<AppId>,
    /// In-memory snapshot of Settings (Launcher owns the canonical copy).
    settings: Settings,
    /// Ticks since last user activity (1 tick = 1 second).
    last_activity_tick: u32,
    /// Whether the screen is currently on.
    screen_on: bool,
}

impl fmt::Debug for Launcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launcher")
            .field("foreground", &self.foreground)
            .field("overlay", &self.overlay)
            .field("screen_on", &self.screen_on)
            .finish_non_exhaustive()
    }
}

impl Launcher {
    pub fn new(settings: Settings, visible_apps: Vec<AppId>) -> Self {
        Self {
            foreground: AppId::Launcher,
            overlay: None,
            visible_apps,
            settings,
            last_activity_tick: 0,
            screen_on: true,
        }
    }

    pub fn foreground(&self) -> AppId {
        self.foreground
    }

    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn screen_on(&self) -> bool {
        self.screen_on
    }

    pub fn visible_apps(&self) -> &[AppId] {
        &self.visible_apps
    }

    /// Launch an app by id (PRD §24: must be in visible_apps).
    pub fn launch(&mut self, app: AppId) -> bool {
        if !self.visible_apps.contains(&app) {
            return false;
        }
        self.foreground = app;
        self.overlay = None;
        self.note_activity();
        true
    }

    /// Back button: dismiss overlay first, else return to Launcher.
    pub fn back(&mut self) {
        if self.overlay.is_some() {
            self.overlay = None;
        } else {
            self.foreground = AppId::Launcher;
        }
        self.note_activity();
    }

    pub fn note_activity(&mut self) {
        self.last_activity_tick = 0;
        self.screen_on = true;
    }

    /// Dispatch an input event with the priority chain (D3).
    pub fn dispatch_input(&mut self, ev: &InputEvent) -> InputDispatch {
        self.note_activity();
        // Overlay takes priority.
        if let Some(overlay) = self.overlay {
            return self.dispatch_to_overlay(overlay, ev);
        }
        // Foreground app gets non-global events.
        match ev {
            InputEvent::PlusShortPress => {
                self.overlay = Some(Overlay::VolumePanel);
                InputDispatch::OpenedOverlay(Overlay::VolumePanel)
            }
            InputEvent::PlusLongPress => {
                self.settings.muted = !self.settings.muted;
                InputDispatch::ToggledMute(self.settings.muted)
            }
            InputEvent::BootShortTap => InputDispatch::ConsumedByLauncher,
            _ => InputDispatch::ForwardedToApp(self.foreground),
        }
    }

    fn dispatch_to_overlay(&mut self, overlay: Overlay, ev: &InputEvent) -> InputDispatch {
        match (overlay, ev) {
            (Overlay::VolumePanel, InputEvent::PlusShortPress) => {
                self.overlay = None;
                InputDispatch::ClosedOverlay
            }
            (Overlay::VolumePanel, InputEvent::Touch(_)) => {
                self.overlay = None;
                InputDispatch::ClosedOverlay
            }
            (Overlay::VolumePanel, InputEvent::PlusLongPress) => {
                self.settings.muted = !self.settings.muted;
                InputDispatch::ToggledMute(self.settings.muted)
            }
            _ => InputDispatch::Consumed(overlay),
        }
    }

    /// Periodic tick (every 1s). Returns the screen action to take.
    pub fn tick(&mut self) -> LauncherTickAction {
        self.last_activity_tick = self.last_activity_tick.saturating_add(1);
        let off_sec = self.settings.screen_off_sec as u32;
        if self.screen_on && self.last_activity_tick >= off_sec {
            self.screen_on = false;
            return LauncherTickAction::ScreenOff;
        }
        LauncherTickAction::None
    }

    /// Wake the screen (called on wake-source event).
    pub fn wake(&mut self) {
        self.screen_on = true;
        self.last_activity_tick = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDispatch {
    ForwardedToApp(AppId),
    OpenedOverlay(Overlay),
    ClosedOverlay,
    ToggledMute(bool),
    ConsumedByLauncher,
    Consumed(Overlay),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTickAction {
    None,
    ScreenOff,
}

// ---- Trait shim ---------------------------------------------------------

pub trait AppShell: Send + Sync + fmt::Debug {
    fn launch(&self, app_id: u8);
    fn current_app(&self) -> u8;
    fn back(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::touch::TouchEvent;
    use crate::services::input::InputEvent;

    fn mk() -> Launcher {
        Launcher::new(Settings::default(), vec![AppId::Settings, AppId::Intercom])
    }

    #[test]
    fn launch_visible_app_succeeds() {
        let mut l = mk();
        assert!(l.launch(AppId::Settings));
        assert_eq!(l.foreground(), AppId::Settings);
    }

    #[test]
    fn launch_invisible_app_fails() {
        let mut l = mk();
        assert!(!l.launch(AppId::About));
        assert_eq!(l.foreground(), AppId::Launcher);
    }

    #[test]
    fn back_dismisses_overlay_first() {
        let mut l = mk();
        l.overlay = Some(Overlay::VolumePanel);
        l.back();
        assert!(l.overlay().is_none());
        l.foreground = AppId::Settings;
        l.back();
        assert_eq!(l.foreground(), AppId::Launcher);
    }

    #[test]
    fn plus_short_press_opens_volume_panel() {
        let mut l = mk();
        let d = l.dispatch_input(&InputEvent::PlusShortPress);
        assert_eq!(d, InputDispatch::OpenedOverlay(Overlay::VolumePanel));
        assert_eq!(l.overlay(), Some(Overlay::VolumePanel));
    }

    #[test]
    fn plus_long_press_toggles_mute() {
        let mut l = mk();
        let initially_muted = l.settings().muted;
        let d = l.dispatch_input(&InputEvent::PlusLongPress);
        assert_eq!(d, InputDispatch::ToggledMute(!initially_muted));
        assert_eq!(l.settings().muted, !initially_muted);
    }

    #[test]
    fn touch_closes_volume_panel() {
        let mut l = mk();
        l.overlay = Some(Overlay::VolumePanel);
        let d = l.dispatch_input(&InputEvent::Touch(TouchEvent::Down { x: 0, y: 0 }));
        assert_eq!(d, InputDispatch::ClosedOverlay);
        assert!(l.overlay().is_none());
    }

    #[test]
    fn non_global_event_forwards_to_app() {
        let mut l = mk();
        l.foreground = AppId::Settings;
        let d = l.dispatch_input(&InputEvent::Touch(TouchEvent::Down { x: 10, y: 20 }));
        assert_eq!(d, InputDispatch::ForwardedToApp(AppId::Settings));
    }

    #[test]
    fn tick_screen_off_after_timeout() {
        let mut l = Launcher::new(
            Settings {
                screen_off_sec: 5,
                ..Settings::default()
            },
            vec![AppId::Settings],
        );
        for _ in 0..4 {
            assert_eq!(l.tick(), LauncherTickAction::None);
        }
        assert_eq!(l.tick(), LauncherTickAction::ScreenOff);
        assert!(!l.screen_on());
    }

    #[test]
    fn activity_resets_screen_off_timer() {
        let mut l = Launcher::new(
            Settings {
                screen_off_sec: 5,
                ..Settings::default()
            },
            vec![AppId::Settings],
        );
        for _ in 0..3 {
            l.tick();
        }
        l.note_activity();
        for _ in 0..4 {
            assert_eq!(l.tick(), LauncherTickAction::None);
        }
    }

    #[test]
    fn wake_resets_state() {
        let mut l = Launcher::new(
            Settings {
                screen_off_sec: 1,
                ..Settings::default()
            },
            vec![AppId::Settings],
        );
        l.tick();
        assert_eq!(l.tick(), LauncherTickAction::ScreenOff);
        l.wake();
        assert!(l.screen_on());
    }
}
