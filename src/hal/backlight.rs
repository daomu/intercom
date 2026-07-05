//! Backlight LEDC PWM driver for GPIO6 (BL pin on Waveshare 1.54 board).
//! 5kHz / 8-bit duty. design D5.
//!
//! NOTE: change 02 stubs the peripheral handle binding. Real LEDC PWM wiring
//! (LedcDriver/LedcTimer/LedcChannel on GPIO6) is added in change 04
//! (PowerService) when on-device brightness ramping is verified. The
//! spec-required `set_brightness/on/off` API is exposed now and tracks the
//! last non-zero brightness in software so `on()` recovery semantics are
//! preserved once LEDC duty is wired up.

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct BacklightDriver {
    last_nonzero_pct: u8,
}

impl BacklightDriver {
    pub fn init() -> Result<Self, HalError> {
        Ok(Self {
            last_nonzero_pct: BoardProfile::DEFAULT_BRIGHTNESS,
        })
    }

    /// Set brightness 0..=100. Persists non-zero value for `on()` recovery.
    pub fn set_brightness(&mut self, v: u8) -> Result<(), HalError> {
        let v = v.min(100);
        if v != 0 {
            self.last_nonzero_pct = v;
        }
        Ok(())
    }

    /// Backlight off.
    pub fn off(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    /// Restore last non-zero brightness (or DEFAULT_BRIGHTNESS).
    pub fn on(&mut self) -> Result<(), HalError> {
        Ok(())
    }

    pub fn last_nonzero_pct(&self) -> u8 {
        self.last_nonzero_pct
    }
}
