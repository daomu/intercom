//! GPIO button driver: BOOT(GPIO9) + PLUS(GPIO18) + PWR(GPIO7=EN, not readable).
//! design D4: ISR pushes raw edge events to a queue, no debounce / classification.
//!
//! NOTE: change 02 stubs the GPIO interrupt + queue wiring. Real PinDriver
//! edge-ISR + FreeRTOS queue construction is added in change 04 (InputService
//! Task C) when the consumer task is created. The `GpioEdgeEvent` enum is
//! defined now so InputService's queue payload type is fixed.
//!
//! PWR button note: BoardProfile::PWR_BTN_PIN=7 is actually the I2C SCL pin
//! on this Waveshare board (see board_profile.rs comment). The Waveshare
//! factory example registers only BOOT+PLUS. PWR long-press is handled in
//! change 04 via BOOT long-press detection. `PwrGpioPress`/`PwrGpioRelease`
//! events are kept in the enum for API completeness but never produced.

#![allow(dead_code)]

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
    _boot_pin: u8,
    _plus_pin: u8,
    _pwr_pin: u8,
}

impl ButtonsDriver {
    pub fn init() -> Result<Self, HalError> {
        // Verify pin constants are referenced (spec: 引脚常量集中).
        let _ = (
            BoardProfile::BOOT_BTN_PIN,
            BoardProfile::PLUS_BTN_PIN,
            BoardProfile::PWR_BTN_PIN,
        );
        Ok(Self {
            _boot_pin: BoardProfile::BOOT_BTN_PIN,
            _plus_pin: BoardProfile::PLUS_BTN_PIN,
            _pwr_pin: BoardProfile::PWR_BTN_PIN,
        })
    }
}
