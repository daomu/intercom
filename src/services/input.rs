//! Input service: button + touch event classification. change 04.
//! design D3-D7. Spec: §3.6, §7.
//!
//! Pure-logic `ButtonClassifier` implements the 50ms PTT / 500ms PLUS long
//! press thresholds with monotonic timestamps; `InputServiceStub` hosts the
//! classifier + callback registration. Real GPIO ISR + CST816 I2C polling
//! land in on-hardware work — the classification logic itself is pure and
//! unit-tested here.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

use crate::hal::buttons::GpioEdgeEvent;
use crate::hal::touch::TouchEvent;

// ---- Thresholds (PRD §7, design D4-D5) ----------------------------------

pub const PTT_PRESS_MS: u64 = 50;
pub const PLUS_LONG_PRESS_MS: u64 = 500;
pub const POWER_SHORT_PRESS_MS: u64 = 2000;

pub const PTT_PRESS: Duration = Duration::from_millis(PTT_PRESS_MS);
pub const PLUS_LONG_PRESS: Duration = Duration::from_millis(PLUS_LONG_PRESS_MS);

// ---- InputEvent ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    BootPress { screen_was_off: bool },
    BootRelease,
    BootShortTap,
    PlusShortPress,
    PlusLongPress,
    PowerShortPress,
    Touch(TouchEvent),
}

// ---- ButtonClassifier: pure-logic edge → InputEvent ---------------------

/// Tracks per-button press timing and emits classified events.
pub struct ButtonClassifier {
    /// Monotonic ms of the last BOOT press edge (0 = not pressed).
    boot_press_ms: u64,
    boot_pressed: bool,
    /// Monotonic ms of the last PLUS press edge.
    plus_press_ms: u64,
    plus_pressed: bool,
    /// Tracks whether we've already emitted BootPress for the current hold
    /// (so release after long-press emits BootRelease, not BootShortTap).
    boot_long_acked: bool,
    plus_long_acked: bool,
    /// Monotonic ms of last PWR press edge.
    pwr_press_ms: u64,
    pwr_pressed: bool,
}

impl fmt::Debug for ButtonClassifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ButtonClassifier")
            .field("boot_pressed", &self.boot_pressed)
            .field("plus_pressed", &self.plus_pressed)
            .field("pwr_pressed", &self.pwr_pressed)
            .finish_non_exhaustive()
    }
}

impl Default for ButtonClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ButtonClassifier {
    pub fn new() -> Self {
        Self {
            boot_press_ms: 0,
            boot_pressed: false,
            plus_press_ms: 0,
            plus_pressed: false,
            boot_long_acked: false,
            plus_long_acked: false,
            pwr_press_ms: 0,
            pwr_pressed: false,
        }
    }

    /// Process a GPIO edge event. `now_ms` is a monotonic timestamp.
    /// `screen_was_off` is read by the caller from ScreenPolicy before invoking.
    /// Returns 0..2 InputEvents (e.g.BootPress on long-press detection while
    /// still held; or BootShortTap on quick release).
    pub fn on_edge(
        &mut self,
        edge: GpioEdgeEvent,
        now_ms: u64,
        screen_was_off: bool,
    ) -> Vec<InputEvent> {
        let mut out = Vec::new();
        match edge {
            GpioEdgeEvent::BootGpioPress => {
                self.boot_press_ms = now_ms;
                self.boot_pressed = true;
                self.boot_long_acked = false;
            }
            GpioEdgeEvent::BootGpioRelease => {
                if self.boot_pressed {
                    let dur = now_ms - self.boot_press_ms;
                    if dur >= PTT_PRESS_MS {
                        // Long press → if we already emitted BootPress, this release is BootRelease;
                        // otherwise emit both BootPress (retroactively) + BootRelease.
                        if !self.boot_long_acked {
                            out.push(InputEvent::BootPress { screen_was_off });
                        }
                        out.push(InputEvent::BootRelease);
                    } else {
                        out.push(InputEvent::BootShortTap);
                    }
                }
                self.boot_pressed = false;
                self.boot_long_acked = false;
            }
            GpioEdgeEvent::PlusGpioPress => {
                self.plus_press_ms = now_ms;
                self.plus_pressed = true;
                self.plus_long_acked = false;
            }
            GpioEdgeEvent::PlusGpioRelease => {
                if self.plus_pressed {
                    let dur = now_ms - self.plus_press_ms;
                    if dur >= PLUS_LONG_PRESS_MS {
                        if !self.plus_long_acked {
                            out.push(InputEvent::PlusLongPress);
                        }
                        // No explicit release event for PLUS.
                    } else {
                        out.push(InputEvent::PlusShortPress);
                    }
                }
                self.plus_pressed = false;
                self.plus_long_acked = false;
            }
            GpioEdgeEvent::PwrGpioPress => {
                self.pwr_press_ms = now_ms;
                self.pwr_pressed = true;
            }
            GpioEdgeEvent::PwrGpioRelease => {
                if self.pwr_pressed {
                    let dur = now_ms - self.pwr_press_ms;
                    if dur < POWER_SHORT_PRESS_MS {
                        out.push(InputEvent::PowerShortPress);
                    }
                    // Long press is hardware bootloader-handled.
                }
                self.pwr_pressed = false;
            }
        }
        out
    }

    /// Called periodically (e.g. every 10ms) to detect mid-hold long-press
    /// thresholds. Emits BootPress when the press crosses 50ms while still
    /// held (so PTT engages immediately, not on release).
    pub fn poll_long_press(&mut self, now_ms: u64, screen_was_off: bool) -> Vec<InputEvent> {
        let mut out = Vec::new();
        if self.boot_pressed && !self.boot_long_acked {
            let dur = now_ms - self.boot_press_ms;
            if dur >= PTT_PRESS_MS {
                out.push(InputEvent::BootPress { screen_was_off });
                self.boot_long_acked = true;
            }
        }
        if self.plus_pressed && !self.plus_long_acked {
            let dur = now_ms - self.plus_press_ms;
            if dur >= PLUS_LONG_PRESS_MS {
                out.push(InputEvent::PlusLongPress);
                self.plus_long_acked = true;
            }
        }
        out
    }
}

// ---- TouchClassifier: screen-off first-touch suppression (D7) -----------

pub struct TouchClassifier {
    /// Whether the first post-wake touch has been consumed.
    first_touch_consumed: bool,
}

impl fmt::Debug for TouchClassifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TouchClassifier")
            .field("first_touch_consumed", &self.first_touch_consumed)
            .finish()
    }
}

impl Default for TouchClassifier {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchClassifier {
    pub fn new() -> Self {
        Self { first_touch_consumed: false }
    }

    /// Process a touch event. `screen_on` is the current ScreenPolicy state.
    /// Returns `Some(InputEvent::Touch(...))` to forward, or `None` if the
    /// event was the wake-first-touch and should be consumed.
    pub fn on_touch(&mut self, e: TouchEvent, screen_on: bool) -> Option<InputEvent> {
        if !screen_on {
            // Screen off — first touch wakes screen, event is consumed.
            self.first_touch_consumed = true;
            return None;
        }
        // Screen on. If this is the first touch after wake, drop it.
        if self.first_touch_consumed {
            self.first_touch_consumed = false;
            return None;
        }
        Some(InputEvent::Touch(e))
    }

    /// Reset state (e.g. on screen-on after wake).
    pub fn reset(&mut self) {
        self.first_touch_consumed = false;
    }
}

// ---- InputService trait + stub ------------------------------------------

pub trait InputService: Send + Sync + fmt::Debug {
    fn on_event(&self, cb: Box<dyn Fn(InputEvent) + Send + Sync>);
}

pub struct InputServiceStub {
    cb: Mutex<Option<Box<dyn Fn(InputEvent) + Send + Sync>>>,
    buttons: Mutex<ButtonClassifier>,
    touch: Mutex<TouchClassifier>,
}

impl fmt::Debug for InputServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputServiceStub").finish_non_exhaustive()
    }
}

impl InputServiceStub {
    pub fn new() -> Self {
        Self {
            cb: Mutex::new(None),
            buttons: Mutex::new(ButtonClassifier::new()),
            touch: Mutex::new(TouchClassifier::new()),
        }
    }

    /// Dispatch a raw GPIO edge event. Reads `screen_was_off` from caller
    /// (ScreenPolicy state) and forwards classified events to the callback.
    pub fn dispatch_edge(&self, edge: GpioEdgeEvent, now_ms: u64, screen_was_off: bool) {
        let events = {
            let mut b = self.buttons.lock().expect("input buttons mutex");
            b.on_edge(edge, now_ms, screen_was_off)
        };
        let cb = self.cb.lock().expect("input cb mutex");
        if let Some(f) = cb.as_ref() {
            for e in events {
                f(e);
            }
        }
    }

    /// Periodic poll for mid-hold long-press detection.
    pub fn poll(&self, now_ms: u64, screen_was_off: bool) {
        let events = {
            let mut b = self.buttons.lock().expect("input buttons mutex");
            b.poll_long_press(now_ms, screen_was_off)
        };
        let cb = self.cb.lock().expect("input cb mutex");
        if let Some(f) = cb.as_ref() {
            for e in events {
                f(e);
            }
        }
    }

    /// Dispatch a touch event.
    pub fn dispatch_touch(&self, e: TouchEvent, screen_on: bool) {
        let event = {
            let mut t = self.touch.lock().expect("input touch mutex");
            t.on_touch(e, screen_on)
        };
        if let Some(e) = event {
            let cb = self.cb.lock().expect("input cb mutex");
            if let Some(f) = cb.as_ref() {
                f(e);
            }
        }
    }
}

impl Default for InputServiceStub {
    fn default() -> Self {
        Self::new()
    }
}

impl InputService for InputServiceStub {
    fn on_event(&self, cb: Box<dyn Fn(InputEvent) + Send + Sync>) {
        let mut slot = self.cb.lock().expect("input cb mutex");
        *slot = Some(cb);
    }
}

unsafe impl Send for InputServiceStub {}
unsafe impl Sync for InputServiceStub {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hal::buttons::GpioEdgeEvent::*;
    use crate::hal::touch::TouchEvent;

    #[test]
    fn boot_short_tap_when_released_under_50ms() {
        let mut c = ButtonClassifier::new();
        let evs = c.on_edge(BootGpioPress, 100, false);
        assert!(evs.is_empty());
        let evs = c.on_edge(BootGpioRelease, 130, false); // 30ms
        assert_eq!(evs, vec![InputEvent::BootShortTap]);
    }

    #[test]
    fn boot_press_engages_at_50ms_while_held() {
        let mut c = ButtonClassifier::new();
        c.on_edge(BootGpioPress, 100, false);
        // poll at 149ms — not yet.
        assert!(c.poll_long_press(149, false).is_empty());
        // poll at 150ms → BootPress emitted.
        let evs = c.poll_long_press(150, false);
        assert_eq!(evs, vec![InputEvent::BootPress { screen_was_off: false }]);
        // Subsequent polls don't re-emit.
        assert!(c.poll_long_press(200, false).is_empty());
        // Release emits BootRelease (not BootShortTap).
        let evs = c.on_edge(BootGpioRelease, 200, false);
        assert_eq!(evs, vec![InputEvent::BootRelease]);
    }

    #[test]
    fn boot_press_carries_screen_was_off() {
        let mut c = ButtonClassifier::new();
        c.on_edge(BootGpioPress, 0, true);
        let evs = c.poll_long_press(60, true);
        assert_eq!(evs, vec![InputEvent::BootPress { screen_was_off: true }]);
    }

    #[test]
    fn plus_short_press_under_500ms() {
        let mut c = ButtonClassifier::new();
        c.on_edge(PlusGpioPress, 0, false);
        let evs = c.on_edge(PlusGpioRelease, 400, false);
        assert_eq!(evs, vec![InputEvent::PlusShortPress]);
    }

    #[test]
    fn plus_long_press_at_500ms() {
        let mut c = ButtonClassifier::new();
        c.on_edge(PlusGpioPress, 0, false);
        let evs = c.poll_long_press(500, false);
        assert_eq!(evs, vec![InputEvent::PlusLongPress]);
    }

    #[test]
    fn power_short_press_under_2s() {
        let mut c = ButtonClassifier::new();
        c.on_edge(PwrGpioPress, 0, false);
        let evs = c.on_edge(PwrGpioRelease, 1000, false);
        assert_eq!(evs, vec![InputEvent::PowerShortPress]);
    }

    #[test]
    fn power_long_press_no_event() {
        let mut c = ButtonClassifier::new();
        c.on_edge(PwrGpioPress, 0, false);
        let evs = c.on_edge(PwrGpioRelease, 3000, false);
        assert!(evs.is_empty(), "long press should be bootloader-handled");
    }

    #[test]
    fn touch_suppressed_when_screen_off() {
        let mut t = TouchClassifier::new();
        let ev = t.on_touch(TouchEvent::Down { x: 10, y: 20 }, false);
        assert!(ev.is_none());
        // First touch after screen-on is also suppressed.
        let ev = t.on_touch(TouchEvent::Down { x: 10, y: 20 }, true);
        assert!(ev.is_none());
        // Subsequent touches forwarded.
        let ev = t.on_touch(TouchEvent::Down { x: 30, y: 40 }, true);
        assert!(matches!(ev, Some(InputEvent::Touch(_))));
    }

    #[test]
    fn touch_forwarded_when_screen_already_on() {
        let mut t = TouchClassifier::new();
        let ev = t.on_touch(TouchEvent::Down { x: 10, y: 20 }, true);
        assert!(matches!(ev, Some(InputEvent::Touch(_))));
    }
}
