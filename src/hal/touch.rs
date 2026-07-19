//! CST816 capacitive touch driver (I2C addr 0x15). design D3.
//!
//! Real I2C wiring: the CST816 sits on the shared I2C bus (SDA=GPIO8 /
//! SCL=GPIO7). The `I2cDriver` is created once in `hal::init()` and passed
//! by `&mut` reference to `read_event()` at call time. This driver stores
//! only the I2C address and tracks previous touch state for Down/Up edge
//! detection.
//!
//! CST816T register map (read 6 bytes starting at reg 0x00):
//!   buf[0] = reg 0x00 GEST_ID  — gesture ID (0x00 for non-gesture taps)
//!   buf[1] = reg 0x01 CTPCR    — touch event (0x04=press, 0x05=lift,
//!                                 0x06=motion); read-to-deassert behavior
//!   buf[2] = reg 0x02 XPOS_H   — low 4 bits = X[11:8]
//!   buf[3] = reg 0x03 XPOS_L   — X[7:0] (single-byte X on 240px panel)
//!   buf[4] = reg 0x04 YPOS_H   — full byte = Y on 240px panel
//!   buf[5] = reg 0x05 YPOS_L   — 0 on 240px panel
//!
//! IMPORTANT (empirically verified on Waveshare ESP32-C6-Touch-LCD-1.54):
//! - `buf[0]` (GEST_ID) is 0x00 for non-gesture touches — do NOT use it as
//!   the "touched" indicator (that was the original bug; the EVENT register
//!   at buf[1] toggles 0x05/0x00 during a press due to read-to-deassert).
//! - Touch is detected by non-zero X/Y coordinates (buf[3] or buf[4]),
//!   which are stable across the entire press.
//! - The touch sensor FPC is mounted with its X axis mirrored relative to
//!   the display origin, so `display_x = LCD_W - raw_x`.
//!
//! IRQ on GPIO5 (falling edge) wiring is deferred to on-device verification;
//! this driver polls the registers when `read_event()` is called.

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
    /// Probes the CST816 and runs the init sequence (motion mask + IRQ polarity
    /// + low-power mode) so the chip reports touch events.
    pub fn init(i2c: &mut I2cDriver<'static>) -> Result<Self, HalError> {
        let _ = (
            BoardProfile::TOUCH_SDA_PIN,
            BoardProfile::TOUCH_SCL_PIN,
            BoardProfile::TOUCH_IRQ_PIN,
            BoardProfile::TOUCH_I2C_ADDR,
        );
        let addr = BoardProfile::TOUCH_I2C_ADDR;
        // Probe: read 6 bytes from register 0x00.
        let mut probe = [0u8; 6];
        match i2c.write_read(addr, &[0x00], &mut probe, BLOCK) {
            Ok(_) => log::info!(
                "CST816 probe OK (addr=0x{:02x}, buf=[{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}])",
                addr, probe[0], probe[1], probe[2], probe[3], probe[4], probe[5]
            ),
            Err(e) => log::warn!(
                "CST816 probe FAILED (addr=0x{:02x}): {} — touch will not work",
                addr, e
            ),
        }
        // Init sequence per CST816T datasheet (spec hal-bsp: "初始化 CST816，
        // 配置 IRQ 中断"). Without this some chip batches won't set the
        // CTPCR event register on touches.
        //   0xFA = MOTION_MASK: enable DOWN | UP | MOTION events
        //   0xFB = IRQ_POL: falling-edge (active-low) IRQ
        //   0xFC = LOW_POWER_MODE: keep chip responsive (no auto-sleep)
        let init_regs: &[(u8, u8)] = &[
            (0xFA, 0x71),
            (0xFB, 0x02),
            (0xFC, 0x10),
        ];
        for &(reg, val) in init_regs {
            match i2c.write(addr, &[reg, val], BLOCK) {
                Ok(_) => log::info!("CST816 init reg 0x{:02x} = {:#04x}", reg, val),
                Err(e) => log::warn!(
                    "CST816 init write reg 0x{:02x}={:#04x} failed: {}",
                    reg, val, e
                ),
            }
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
        //   [0] GEST_ID, [1] CTPCR/event, [2] X_H, [3] X_L, [4] Y_H, [5] Y_L
        let mut buf = [0u8; 6];
        i2c.write_read(self.addr, &[0x00], &mut buf, BLOCK)
            .map_err(|e| HalError::TouchInitFailed(format!("I2C read: {e}")))?;

        // The CTPCR register (buf[1]) has read-to-deassert behavior (toggles
        // 0x05/0x00 during a single press), so it is NOT a reliable "finger
        // present" signal. Touch is detected from non-zero X/Y coordinates,
        // which remain stable across the entire press.
        //   buf[0] = GEST_ID  (0x00 for non-gesture)
        //   buf[1] = CTPCR    (event code, read-to-deassert)
        //   buf[2] = X_H      (high nibble of X; 0 on 240px panel)
        //   buf[3] = X_L      (single-byte X coordinate)
        //   buf[4] = Y        (single-byte Y coordinate on 240px panel)
        //   buf[5] = 0
        let event_byte = buf[1];
        let raw_x = buf[3] as u16;
        let raw_y = buf[4] as u16;
        let touched = raw_x != 0 || raw_y != 0;

        // The touch sensor FPC on this board is mounted with its X origin at
        // the right edge (mirrored relative to the display origin at the
        // left edge), so flip X to map a touch into display coordinates.
        let x = if touched { BoardProfile::LCD_W as u16 - raw_x } else { 0 };
        let y = raw_y;

        // Diagnostic: log only on state transitions (new touch / release)
        // to avoid flooding while the finger is held still. Idle polls are
        // throttled to 1 per ~5s (100 polls × 50ms). Down/Swipe/Up dispatch
        // is already logged by main.rs's "Dispatching touch" line, so the
        // driver only needs to surface the raw register dump on edges.
        self.poll_count = self.poll_count.wrapping_add(1);
        let is_edge = touched != self.was_touched;
        if is_edge {
            log::info!(
                "CST816 {}: event={:#04x}, x={}, y={}, buf=[{:#04x},{:#04x},{:#04x},{:#04x},{:#04x},{:#04x}]",
                if touched { "press" } else { "release" },
                event_byte, x, y, buf[0], buf[1], buf[2], buf[3], buf[4], buf[5]
            );
        } else if !touched && self.poll_count % 100 == 0 {
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
                // Finger held still — no new event. Return the idle sentinel
                // (Up { 0, 0 }) which the main loop filters out before
                // dispatch. Returning Down here would flood dispatch with a
                // Down every 50ms while the finger rests (and would toggle
                // PTT on every poll once PTT is wired).
                TouchEvent::Up { x: 0, y: 0 }
            }
        } else {
            // No touch, was not touched → idle Up.
            TouchEvent::Up { x: 0, y: 0 }
        };

        self.was_touched = touched;
        Ok(event)
    }
}
