//! Input service: button + touch event classification. change 04/17.
//! design D3-D7. Spec: §3.6, §7.
//!
//! NOTE: change 04 ships the trait + event enums + stub impl. Real GPIO
//! edge-ISR consumption + CST816 polling + 50ms/500ms timers land when
//! on-device verification is possible. Build-time acceptance: cargo build.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;

use crate::hal::buttons::GpioEdgeEvent;
use crate::hal::touch::TouchEvent;

/// Classified input event. design D3-D7, PRD §7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// BOOT pressed >= 50ms (PTT engage). `screen_was_off` lets change 10
    /// decide whether to keep screen off during PTT (D45 complementary).
    BootPress { screen_was_off: bool },
    /// BOOT released after BootPress.
    BootRelease,
    /// BOOT tapped < 50ms (wake screen only, no PTT).
    BootShortTap,
    /// PLUS pressed < 500ms (volume panel).
    PlusShortPress,
    /// PLUS held >= 500ms (mute toggle).
    PlusLongPress,
    /// PWR short press (< 2s). Long-press is hardware bootloader-handled.
    PowerShortPress,
    /// CST816 touch event.
    Touch(TouchEvent),
}

/// Input service trait. design D3: callback registration pattern.
pub trait InputService: Send + Sync + fmt::Debug {
    /// Register a single callback. Replaces any prior callback.
    fn on_event(&self, cb: Box<dyn Fn(InputEvent) + Send + Sync>);
}

/// Stub impl. Holds callback in Mutex; classification logic (50ms/500ms
/// timers, screen-off first-touch suppression) lands in on-hardware work.
pub struct InputServiceStub {
    cb: Mutex<Option<Box<dyn Fn(InputEvent) + Send + Sync>>>,
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
        }
    }

    /// Stub: classify a raw GPIO edge event into InputEvent. Real impl uses
    /// 50ms/500ms timers; this stub emits BootPress/BootRelease directly.
    #[allow(dead_code)]
    pub fn classify_edge(_e: GpioEdgeEvent, _screen_was_off: bool) -> Option<InputEvent> {
        // TODO: real classification with 50ms PTT threshold (design D4).
        None
    }

    /// Stub: handle a touch event with screen-off first-touch suppression.
    #[allow(dead_code)]
    pub fn classify_touch(_e: TouchEvent, _screen_on: bool) -> Option<InputEvent> {
        // TODO: design D7 — if screen off, first touch only wakes screen.
        None
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
