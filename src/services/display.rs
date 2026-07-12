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

use embedded_graphics::geometry::Point;
use embedded_graphics::mono_font::ascii::FONT_6X9;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;

use crate::board_profile::BoardProfile;
use crate::hal::backlight::BacklightDriver;
use crate::hal::lcd::LcdDriver;
use crate::hal::HalError;
use crate::services::display_buf::Rgb565Buf;

/// Drawing command. design D11.
#[derive(Debug, Clone, Copy)]
pub enum DrawCmd {
    SlintUpdate,
    Clear,
    RawFramebuffer(&'static [u8]),
    /// Re-push the current framebuffer to the LCD as-is, without
    /// re-rendering. Used after `screen_on()` to restore the retained
    /// framebuffer image on wake.
    Redraw,
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

// ---- Real HAL impl (LEDC PWM + ST7789 framebuffer pump) ------------------

/// Holds the LCD driver and its framebuffer together so `present()` can
/// draw into the buffer and push it to the panel under a single lock.
struct DisplayInner {
    lcd: LcdDriver,
    fb: Rgb565Buf,
}

/// Real display service: LEDC backlight PWM + ST7789 framebuffer pump via
/// `embedded-graphics`. `present()` translates `DrawCmd` into framebuffer
/// fills / direct pushes. `draw_boot_screen()` renders a startup banner.
pub struct HalDisplayService {
    backlight: Mutex<BacklightDriver>,
    inner: Mutex<DisplayInner>,
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
    /// Construct from an owned `BacklightDriver` + `LcdDriver` (moved out of
    /// `Hal`). Allocates a `LCD_W × LCD_H` RGB565 framebuffer on the heap.
    pub fn new(backlight: BacklightDriver, lcd: LcdDriver) -> Self {
        let fb = Rgb565Buf::new(BoardProfile::LCD_W, BoardProfile::LCD_H);
        Self {
            backlight: Mutex::new(backlight),
            inner: Mutex::new(DisplayInner { lcd, fb }),
            brightness: Mutex::new(BoardProfile::DEFAULT_BRIGHTNESS),
            state: Mutex::new(ScreenState::On),
        }
    }

    /// Render the boot screen (dark background + version text) into the
    /// framebuffer and push it to the LCD once. Called at startup before
    /// entering the main loop, to eliminate the post-init garbage screen.
    pub fn draw_boot_screen(&self) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("DisplayInner lock poisoned: {e}");
                return;
            }
        };
        // Dark navy background.
        let bg = Rgb565::new(0x04, 0x02, 0x10); // ~#040410-ish in RGB565
        inner.fb.fill(bg);
        // White centered title + version.
        let style = MonoTextStyle::new(&FONT_6X9, Rgb565::WHITE);
        let title = "Intercom";
        let ver = concat!("v", env!("CARGO_PKG_VERSION"));
        // Center text horizontally: FONT_6X9 width = 6px per char.
        let title_w = (title.len() as u32) * 6;
        let ver_w = (ver.len() as u32) * 6;
        let cx = (BoardProfile::LCD_W as i32 - title_w as i32) / 2;
        let vx = (BoardProfile::LCD_W as i32 - ver_w as i32) / 2;
        let _ = Text::new(title, Point::new(cx, BoardProfile::LCD_H as i32 / 2 - 4), style)
            .draw(&mut inner.fb);
        let _ = Text::new(ver, Point::new(vx, BoardProfile::LCD_H as i32 / 2 + 10), style)
            .draw(&mut inner.fb);
        let DisplayInner { lcd, fb } = &mut *inner;
        if let Err(e) = lcd.present(fb.as_bytes()) {
            log::error!("Boot screen LCD push failed: {e:?}");
        }
    }

    /// Give view layer direct write access to the framebuffer, then
    /// automatically push the framebuffer to the LCD. This is the single
    /// render→present entry point used by the main loop.
    pub fn with_fb<F>(&self, f: F) -> Result<(), HalError>
    where
        F: FnOnce(&mut Rgb565Buf),
    {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| HalError::LcdInitFailed(format!("DisplayInner lock: {e}")))?;
        let inner = &mut *inner;
        f(&mut inner.fb);
        inner.lcd.present(inner.fb.as_bytes())
    }

    /// Whether the screen is currently on (controls whether the main loop
    /// calls `with_fb`).
    pub fn is_screen_on(&self) -> bool {
        *self.state.lock().unwrap() == ScreenState::On
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
    fn present(&self, cmd: &DrawCmd) {
        match cmd {
            DrawCmd::Clear => {
                if let Ok(mut inner) = self.inner.lock() {
                    inner.fb.fill(Rgb565::BLACK);
                    let DisplayInner { lcd, fb } = &mut *inner;
                    let _ = lcd.present(fb.as_bytes());
                }
            }
            DrawCmd::RawFramebuffer(data) => {
                if let Ok(mut inner) = self.inner.lock() {
                    let _ = inner.lcd.present(data);
                }
            }
            DrawCmd::SlintUpdate => {
                log::warn!("SlintUpdate ignored — slint runtime backend not wired");
            }
            DrawCmd::Redraw => {
                if let Ok(mut inner) = self.inner.lock() {
                    let DisplayInner { lcd, fb } = &mut *inner;
                    // Re-push the retained framebuffer without re-rendering.
                    if let Err(e) = lcd.present(fb.as_bytes()) {
                        log::error!("Redraw LCD push failed: {e:?}");
                    }
                }
            }
        }
    }
}

unsafe impl Send for HalDisplayService {}
unsafe impl Sync for HalDisplayService {}
