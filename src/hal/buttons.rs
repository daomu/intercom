//! GPIO button driver: BOOT(GPIO9) + PLUS(GPIO18) + PWR(GPIO7=EN, not readable).
//! design D4: ISR pushes raw edge events to a queue, no debounce / classification.
//!
//! Real GPIO wiring: PinDriver in input mode with pull-up on BOOT + PLUS.
//! PWR button (GPIO7) is the I2C SCL pin on this board — not available as a
//! digital input. PWR long-press is handled via BOOT long-press detection in
//! InputService.
//!
//! Edge interrupt + FreeRTOS queue construction is deferred to on-device
//! verification. This driver provides polling-based state reads
//! (`boot_pressed()`, `plus_pressed()`) so InputService can sample and
//! classify. The `GpioEdgeEvent` enum is kept for the future ISR path.

#![allow(dead_code)]

use esp_idf_svc::hal::gpio::{Input, InputPin, PinDriver, Pull};

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

/// Raw GPIO edge event pushed from ISR. change 04 InputService classifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioEdgeEvent {
    BootGpioPress,
    BootGpioRelease,
    PlusGpioPress,
    PlusGpioRelease,
    PwrGpioPress,
    PwrGpioRelease,
}

pub struct ButtonsDriver {
    boot: PinDriver<'static, Input>,
    plus: PinDriver<'static, Input>,
}

impl std::fmt::Debug for ButtonsDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ButtonsDriver")
            .field("boot_pin", &BoardProfile::BOOT_BTN_PIN)
            .field("plus_pin", &BoardProfile::PLUS_BTN_PIN)
            .finish_non_exhaustive()
    }
}

impl ButtonsDriver {
    /// Construct from owned BOOT + PLUS GPIO pins (input, pull-up).
    pub fn init<BOOTPIN, PLUSPIN>(boot_pin: BOOTPIN, plus_pin: PLUSPIN) -> Result<Self, HalError>
    where
        BOOTPIN: InputPin + 'static,
        PLUSPIN: InputPin + 'static,
    {
        let boot = PinDriver::input(boot_pin, Pull::Up)
            .map_err(|e| HalError::ButtonsInitFailed(format!("BOOT pin: {e}")))?;
        let plus = PinDriver::input(plus_pin, Pull::Up)
            .map_err(|e| HalError::ButtonsInitFailed(format!("PLUS pin: {e}")))?;
        Ok(Self { boot, plus })
    }

    /// BOOT button (GPIO9) pressed = logic-low (pull-up + button-to-GND).
    pub fn boot_pressed(&self) -> bool {
        self.boot.is_low()
    }

    /// PLUS button (GPIO18) pressed = logic-low (pull-up + button-to-GND).
    pub fn plus_pressed(&self) -> bool {
        self.plus.is_low()
    }
}
