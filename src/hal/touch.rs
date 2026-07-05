//! CST816 capacitive touch driver (I2C addr 0x15). design D3.
//!
//! Real I2C wiring: the CST816 sits on the shared I2C bus (SDA=GPIO8 /
//! SCL=GPIO7). The `I2cDriver` is created once in `hal::init()` and passed
//! by `&mut` reference to `read_event()`. This driver stores only the I2C
//! address and tracks previous touch state for Down/Up edge detection.
//!
//! CST816T register map (relevant subset):
//!   0x03 FINGERNUM — bit 3..0 = finger count (0 = no touch, 1 = one finger)
//!   0x04 XPOS_H    — lower 4 bits are X[11:8]
//!   0x05 XPOS_L    — X[7:0]
//!   0x06 YPOS_H    — lower 4 bits are Y[11:8]
//!   0x07 YPOS_L    — Y[7:0]
//!
//! IRQ on GPIO5 (falling edge) wiring is deferred to on-device verification;
//! this driver polls the FINGERNUM register when `read_event()` is called.

#![allow(dead_code)]

use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::i2c::I2cDriver;

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
    addr: u8,
    was_touched: bool,
    last_x: u16,
    last_y: u16,
}

impl std::fmt::Debug for TouchDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TouchDriver")
            .field("addr", &self.addr)
            .field("was_touched", &self.was_touched)
            .finish_non_exhaustive()
    }
}

impl TouchDriver {
    /// Construct with the CST816 I2C address. The I2C bus driver is owned
    /// by `Hal` and passed to `read_event()` at call time (shared bus).
    pub fn init() -> Result<Self, HalError> {
        // Verify BoardProfile pin constants are referenced (spec: 引脚常量集中).
        let _ = (
            BoardProfile::TOUCH_SDA_PIN,
            BoardProfile::TOUCH_SCL_PIN,
            BoardProfile::TOUCH_IRQ_PIN,
            BoardProfile::TOUCH_I2C_ADDR,
        );
        Ok(Self {
            addr: BoardProfile::TOUCH_I2C_ADDR,
            was_touched: false,
            last_x: 0,
            last_y: 0,
        })
    }

    /// Poll the CST816 for a touch event. Returns `Ok(Up)` when no change.
    /// Caller passes the shared `I2cDriver` (split-borrow from `Hal`).
    pub fn read_event(&mut self, i2c: &mut I2cDriver<'static>) -> Result<TouchEvent, HalError> {
        // Read 5 bytes starting at register 0x03: FINGERNUM, X_H, X_L, Y_H, Y_L.
        let mut buf = [0u8; 5];
        i2c.write_read(self.addr, &[0x03], &mut buf, BLOCK)
            .map_err(|e| HalError::TouchInitFailed(format!("I2C read: {e}")))?;

        let finger = buf[0] & 0x0F;
        let x = ((buf[1] as u16 & 0x0F) << 8) | buf[2] as u16;
        let y = ((buf[3] as u16 & 0x0F) << 8) | buf[4] as u16;
        let touched = finger != 0;

        let event = if touched && !self.was_touched {
            // New touch → Down event.
            self.last_x = x;
            self.last_y = y;
            TouchEvent::Down { x, y }
        } else if !touched && self.was_touched {
            // Release → Up event at last known position.
            let ev = TouchEvent::Up { x: self.last_x, y: self.last_y };
            self.last_x = 0;
            self.last_y = 0;
            ev
        } else if touched && self.was_touched {
            // Continued touch — detect swipe if position moved enough.
            let dx = x as i16 - self.last_x as i16;
            let dy = y as i16 - self.last_y as i16;
            self.last_x = x;
            self.last_y = y;
            if dx.abs() > 20 || dy.abs() > 20 {
                TouchEvent::Swipe { dx, dy }
            } else {
                TouchEvent::Down { x, y } // small movement, treat as Down
            }
        } else {
            // No touch, was not touched → idle Up.
            TouchEvent::Up { x: 0, y: 0 }
        };

        self.was_touched = touched;
        Ok(event)
    }
}
