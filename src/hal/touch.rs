//! CST816 capacitive touch driver (I2C addr 0x15). design D3.
//!
//! Real I2C wiring: the CST816 sits on the shared I2C bus (SDA=GPIO8 /
//! SCL=GPIO7). The `I2cDriver` is created once in `hal::init()` and passed
//! by `&mut` reference to `read_event()` at call time. This driver stores
//! only the I2C address and tracks previous touch state for Down/Up edge
//! detection.
//!
//! CST816 register map (chip ID 0xB6 at reg 0xA7 — CST816T variant):
//!   0x00 EVENT   — 0x00=none, 0x04=press, 0x05=lift, 0x06=contact/move
//!   0x01 FINGER  — finger id (0 = first finger)
//!   0x02 XPOS_H  — lower 4 bits are X[11:8]
//!   0x03 XPOS_L  — X[7:0]
//!   0x04 YPOS_H  — lower 4 bits are Y[11:8]
//!   0x05 YPOS_L  — Y[7:0]
//!
//! IRQ on GPIO5 (falling edge) wiring is deferred to on-device verification;
//! this driver polls the EVENT register when `read_event()` is called.
//!
//! NOTE: The EVENT register may have read-to-clear behavior — the event
//! value is consumed on read. Polling at 50ms should be sufficient for
//! normal tap detection.

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
    /// Diagnostic counter for periodic raw-value logging.
    poll_count: u32,
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
    /// Probes the CST816 at init time to verify I2C communication.
    pub fn init(i2c: &mut I2cDriver<'static>) -> Result<Self, HalError> {
        let _ = (
            BoardProfile::TOUCH_SDA_PIN,
            BoardProfile::TOUCH_SCL_PIN,
            BoardProfile::TOUCH_IRQ_PIN,
            BoardProfile::TOUCH_I2C_ADDR,
        );
        let addr = BoardProfile::TOUCH_I2C_ADDR;
        // Simple probe: read 6 bytes from register 0x00.
        let mut probe = [0u8; 6];
        match i2c.write_read(addr, &[0x00], &mut probe, BLOCK) {
            Ok(_) => log::info!(
                "CST816 probe OK (addr=0x{:02x}, event={}, buf=[{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}])",
                addr, probe[0], probe[0], probe[1], probe[2], probe[3], probe[4], probe[5]
            ),
            Err(e) => log::warn!(
                "CST816 probe FAILED (addr=0x{:02x}): {} — touch will not work",
                addr, e
            ),
        }
        Ok(Self {
            addr,
            was_touched: false,
            last_x: 0,
            last_y: 0,
            poll_count: 0,
        })
    }

    /// Poll the CST816 for a touch event. Returns `Ok(Up { x: 0, y: 0 })`
    /// when no touch activity. Caller passes the shared `I2cDriver`.
    pub fn read_event(&mut self, i2c: &mut I2cDriver<'static>) -> Result<TouchEvent, HalError> {
        // Read 6 bytes starting at register 0x00 (CST816T layout):
        //   [0] EVENT, [1] FINGER_ID, [2] X_H, [3] X_L, [4] Y_H, [5] Y_L
        let mut buf = [0u8; 6];
        i2c.write_read(self.addr, &[0x00], &mut buf, BLOCK)
            .map_err(|e| HalError::TouchInitFailed(format!("I2C read: {e}")))?;

        let event_byte = buf[0];
        let x = ((buf[2] as u16 & 0x0F) << 8) | buf[3] as u16;
        let y = ((buf[4] as u16 & 0x0F) << 8) | buf[5] as u16;

        let touched = event_byte != 0;

        // Diagnostic: log every ~5s (100 polls × 50ms) even when no touch,
        // and immediately on any touch event.
        self.poll_count = self.poll_count.wrapping_add(1);
        if touched {
            log::info!(
                "CST816: event={:#04x}, x={}, y={}, buf=[{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}]",
                event_byte, x, y, buf[0], buf[1], buf[2], buf[3], buf[4], buf[5]
            );
        } else if self.poll_count % 100 == 0 {
            log::info!(
                "CST816 idle poll: buf=[{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}]",
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5]
            );
        }

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
