//! Display service: backlight PWM + screen on/off + slint present. change 04/17.
//! design D1, D2, D11. Spec: §3.6.
//!
//! Two implementations:
//! - `DisplayServiceStub`: pure software state (no hardware), for host tests.
//! - `HalDisplayService`: real LEDC PWM via `BacklightDriver`, for on-device.
//!
//! The slint runtime backend + ST7789 frame pump (`present()` method) remain
//! deferred — slint-on-ESP-IDF needs fontique/memmap2 patches. Build-time
//! acceptance: cargo build.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;

use crate::board_profile::BoardProfile;
use crate::hal::backlight::BacklightDriver;

/// Drawing command. design D11.
#[derive(Debug, Clone, Copy)]
pub enum DrawCmd {
    SlintUpdate,
    Clear,
    RawFramebuffer(&'static [u8]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenState {
    On,
    Off,
}

/// Display service trait. §3.6 / design D1-D2.
pub trait DisplayService: Send + Sync + fmt::Debug {
    fn set_brightness(&self, v: u8);
    fn screen_on(&self);
    fn screen_off(&self);
    fn is_screen_on(&self) -> bool;
    fn present(&self, cmd: &DrawCmd);
}

// ---- Stub impl (no hardware, for host tests) -----------------------------

pub struct DisplayServiceStub {
    brightness: u8,
    state: ScreenState,
}

impl fmt::Debug for DisplayServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DisplayServiceStub")
            .field("brightness", &self.brightness)
            .field("state", &self.state)
            .finish()
    }
}

impl DisplayServiceStub {
    pub fn new() -> Self {
        Self {
            brightness: BoardProfile::DEFAULT_BRIGHTNESS,
            state: ScreenState::On,
        }
    }
}

impl Default for DisplayServiceStub {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayService for DisplayServiceStub {
    fn set_brightness(&self, _v: u8) {}
    fn screen_on(&self) {}
    fn screen_off(&self) {}
    fn is_screen_on(&self) -> bool {
        self.state == ScreenState::On
    }
    fn present(&self, _cmd: &DrawCmd) {}
}

unsafe impl Send for DisplayServiceStub {}
unsafe impl Sync for DisplayServiceStub {}

// ---- Real HAL impl (LEDC PWM via BacklightDriver) ------------------------

/// Real display service backed by `BacklightDriver` (LEDC PWM on GPIO6).
/// The `present()` method is a no-op until the slint runtime backend is wired.
pub struct HalDisplayService {
    backlight: Mutex<BacklightDriver>,
    brightness: Mutex<u8>,
    state: Mutex<ScreenState>,
}

impl fmt::Debug for HalDisplayService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HalDisplayService")
            .field("brightness", &self.brightness)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl HalDisplayService {
    /// Construct from an owned `BacklightDriver` (moved from `Hal`).
    pub fn new(backlight: BacklightDriver) -> Self {
        Self {
            backlight: Mutex::new(backlight),
            brightness: Mutex::new(BoardProfile::DEFAULT_BRIGHTNESS),
            state: Mutex::new(ScreenState::On),
        }
    }
}

impl DisplayService for HalDisplayService {
    fn set_brightness(&self, v: u8) {
        *self.brightness.lock().unwrap() = v.min(100);
        if let Ok(mut bl) = self.backlight.lock() {
            let _ = bl.set_brightness(v);
        }
    }
    fn screen_on(&self) {
        *self.state.lock().unwrap() = ScreenState::On;
        if let Ok(mut bl) = self.backlight.lock() {
            let _ = bl.on();
        }
    }
    fn screen_off(&self) {
        *self.state.lock().unwrap() = ScreenState::Off;
        if let Ok(mut bl) = self.backlight.lock() {
            let _ = bl.off();
        }
    }
    fn is_screen_on(&self) -> bool {
        *self.state.lock().unwrap() == ScreenState::On
    }
    fn present(&self, _cmd: &DrawCmd) {
        // TODO: slint backend refresh + ST7789 framebuffer push via LcdDriver.
    }
}

unsafe impl Send for HalDisplayService {}
unsafe impl Sync for HalDisplayService {}
