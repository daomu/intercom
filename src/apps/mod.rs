//! Application layer: App trait + view-layer render types + submodules.
//!
//! change 07 defined the `Launcher` / `SettingsApp` / `IntercomApp` pure-logic
//! state machines. This module adds the `App` trait with `render()` /
//! `hit_test()` extensions (embedded-graphics command-mode rendering replaces
//! slint's declarative redraw), plus `RenderCtx` snapshot, `UiEvent` queue,
//! and `HitTarget` for procedural touch hit-testing.

#![allow(dead_code)]

pub mod intercom_app;
pub mod settings;
pub mod shell;
pub mod view;

use std::sync::{Arc, Mutex};

use crate::intercom::state::IntercomMode;
use crate::services::display_buf::Rgb565Buf;
use crate::services::input::InputEvent;
use crate::services::storage::Settings;
use crate::services::power::ResetReason;

/// Ambient snapshot of service state for view rendering. Built by the main
/// loop each tick so view functions read a value (no `Mutex` lock during draw).
pub struct RenderCtx<'a> {
    /// Battery level 0-3 (4-step icon). 0 = critical, 3 = full.
    pub battery_step: u8,
    /// Signal strength 0-4 (bars). 0 = no radio / ungrouped.
    pub signal_bars: u8,
    /// Local time (hour, minute, second). (0,0,0) if no RTC/NTP.
    pub time_hms: (u8, u8, u8),
    /// Whether the device has joined a group (`StorageService::load_group().is_some()`).
    pub is_grouped: bool,
    /// Global mute flag from `Settings.muted`.
    pub muted: bool,
    /// Firmware version string (compile-time constant).
    pub fw_version: &'static str,
    /// Current settings (device name / volume / brightness / screen_off_sec / ...).
    pub settings: &'a Settings,
    /// Safe-boot mode flag (from `DiagInfo.safe_boot_flag`).
    pub safe_mode: bool,
    /// Current intercom mode when grouped (None if ungrouped). Used by
    /// StatusBar mode icon + IntercomView.
    pub mode: Option<IntercomMode>,
    /// Build timestamp (epoch seconds, for About page).
    pub build_time: u64,
    /// Last reset reason (for About page).
    pub reset_reason: ResetReason,
    /// Abnormal boot count (for About page).
    pub abnormal_boot_count: u32,
}

/// Cross-thread UI event (replaces `slint::invoke_from_event_loop`).
/// Producers (ESP-NOW / audio threads) push; main loop drains each tick.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// Generic "redraw requested" — set when any model state changes.
    Dirty,
    /// Intercom event from ESP-NOW thread. Placeholder `()` until the real
    /// `IntercomEvent` type is delivered by its change; drained events are
    /// logged and ignored (no panic).
    Intercom(()),
    /// Network event. Placeholder `()` until the real `NetworkEvent` type
    /// is delivered.
    Network(()),
    /// Audio event. Placeholder `()` until the real `AudioEvent` type is
    /// delivered.
    Audio(()),
}

/// Bounded UI event queue shared between producer threads and the main loop.
pub type UiEventQueue = Arc<Mutex<std::collections::VecDeque<UiEvent>>>;

/// Create a new UI event queue with the given capacity hint.
pub fn new_ui_event_queue() -> UiEventQueue {
    Arc::new(Mutex::new(std::collections::VecDeque::with_capacity(64)))
}

/// Push a UI event onto the queue (producer side, non-blocking).
/// If the queue is full (≥ 64), the event is dropped + logged.
pub fn push_ui_event(queue: &UiEventQueue, ev: UiEvent) {
    if let Ok(mut q) = queue.lock() {
        if q.len() >= 64 {
            log::warn!("UiEvent queue full, dropping {:?}", ev);
            return;
        }
        q.push_back(ev);
    }
}

/// Drain all pending UI events (consumer side). Returns whether any events
/// were drained (caller sets `dirty = true` if so). Placeholder variants
/// (`Intercom(())` / `Network(())` / `Audio(())`) are logged and ignored —
/// they do not panic; once the real event types are delivered by their
/// changes, dispatch logic will replace the log-and-ignore path.
pub fn drain_ui_events(queue: &UiEventQueue) -> bool {
    let mut any = false;
    if let Ok(mut q) = queue.lock() {
        while let Some(ev) = q.pop_front() {
            any = true;
            match &ev {
                UiEvent::Dirty => {}
                UiEvent::Intercom(_) => {
                    log::debug!("UiEvent::Intercom placeholder drained (real type pending)");
                }
                UiEvent::Network(_) => {
                    log::debug!("UiEvent::Network placeholder drained (real type pending)");
                }
                UiEvent::Audio(_) => {
                    log::debug!("UiEvent::Audio placeholder drained (real type pending)");
                }
            }
        }
    }
    any
}

/// Touch hit-test result. Each view defines its own targets via this enum;
/// the controller translates targets into model actions (launch app / flip
/// page / toggle setting / ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTarget {
    /// Launcher home tile: Intercom entry.
    LauncherIntercomTile,
    /// Launcher home tile: Settings entry.
    LauncherSettingsTile,
    /// Settings page navigation (left = prev page, right = next page).
    SettingsPageNav { forward: bool },
    /// A settings value control was tapped (which field TBD by view).
    SettingsControl { field: u8 },
    /// Intercom bottom PTT area.
    IntercomPttArea,
    /// Intercom page navigation (left/right swipe target).
    IntercomPageNav { forward: bool },
    /// Volume panel: mute button.
    VolumeMuteBtn,
    /// Volume panel: close (tap on mask).
    VolumePanelClose,
    /// No hit (background).
    None,
}

/// App trait: lifecycle + command-mode render + procedural hit-test.
///
/// This extends change 07's behavioral trait (id/title/on_enter/on_exit/
/// on_event/on_tick) with `render()` and `hit_test()` for embedded-graphics
/// command-mode rendering. Apps own their model state and draw themselves.
pub trait App: Send + std::fmt::Debug {
    /// Stable app identifier (matches `AppId::as_str`).
    fn id(&self) -> &str;
    /// Human-readable title (for Launcher tile label).
    fn title(&self) -> &str;

    fn on_enter(&mut self, ctx: &AppContext);
    fn on_exit(&mut self, ctx: &AppContext);
    fn on_event(&mut self, ev: &InputEvent, ctx: &AppContext);
    fn on_tick(&mut self, ctx: &AppContext);

    /// Command-mode render: draw current app state into `fb` using `ctx`.
    fn render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx);

    /// Procedural touch hit-test. Returns the hit target at `(x, y)` given
    /// current layout state, or `None` if the point is background.
    fn hit_test(&self, x: i32, y: i32, ctx: &RenderCtx) -> Option<HitTarget>;
}

/// Service injection context for App lifecycle callbacks.
pub struct AppContext<'a> {
    pub storage: &'a dyn crate::services::storage::StorageService,
    pub display: &'a dyn crate::services::display::DisplayService,
    pub power: &'a dyn crate::services::power::PowerService,
    pub settings: &'a Settings,
}
