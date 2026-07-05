//! Display service: backlight PWM + screen on/off + slint present. change 04/17.
//! design D1, D2, D11. Spec: §3.6.
//!
//! NOTE: change 04 ships the trait + stub impl. Real LEDC PWM wiring on
//! GPIO6 + slint backend present() land when the slint platform backend is
//! wired up (change 02 deferred slint runtime to here, but on-device
//! slint-on-ESP-IDF still needs fontique/memmap2 patches + ST7789 frame
//! pump — left as on-hardware work). Build-time acceptance: cargo build.

#![allow(dead_code)]

use std::fmt;

use crate::board_profile::BoardProfile;

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

/// Stub impl. Holds brightness + screen state in software; real LEDC PWM
/// wiring lands when on-device slint backend is verified.
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
    fn set_brightness(&self, _v: u8) {
        // TODO: LEDC duty = (v * 255) / 100 on GPIO6.
    }
    fn screen_on(&self) {
        // TODO: restore LEDC duty + raise screen_on flag.
    }
    fn screen_off(&self) {
        // TODO: LEDC duty = 0.
    }
    fn is_screen_on(&self) -> bool {
        self.state == ScreenState::On
    }
    fn present(&self, _cmd: &DrawCmd) {
        // TODO: slint backend refresh on SlintUpdate; framebuffer clear on Clear.
    }
}

// Allow interior mutability pattern in real impl; stub uses &self for trait
// compatibility with future Arc<dyn DisplayService> sharing.
unsafe impl Send for DisplayServiceStub {}
unsafe impl Sync for DisplayServiceStub {}
