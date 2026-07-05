//! CST816 capacitive touch driver (I2C addr 0x15). design D3.
//!
//! NOTE: change 02 stubs I2C + IRQ wiring. Real CST816 init (I2cDriver on
//! SDA=GPIO8/SCL=GPIO7, IRQ on GPIO5 falling edge, register read sequence)
//! is added in change 04 (InputService) when touch events feed the input
//! pipeline. The `TouchEvent` enum is defined now so InputService's mapping
//! target is fixed.

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

/// Raw CST816 event. change 04 InputService maps these to `InputEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEvent {
    Down { x: u16, y: u16 },
    Up { x: u16, y: u16 },
    Swipe { dx: i16, dy: i16 },
}

pub struct TouchDriver {
    _addr: u8,
}

impl TouchDriver {
    pub fn init() -> Result<Self, HalError> {
        // Verify BoardProfile pin constants are referenced (spec: 引脚常量集中).
        let _ = (
            BoardProfile::TOUCH_SDA_PIN,
            BoardProfile::TOUCH_SCL_PIN,
            BoardProfile::TOUCH_IRQ_PIN,
            BoardProfile::TOUCH_I2C_ADDR,
        );
        Ok(Self {
            _addr: BoardProfile::TOUCH_I2C_ADDR,
        })
    }

    /// Read one touch event. Stubbed — returns `Up(0,0)` when no data.
    pub fn read_event(&mut self) -> Result<TouchEvent, HalError> {
        Ok(TouchEvent::Up { x: 0, y: 0 })
    }
}
