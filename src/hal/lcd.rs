//! ST7789 LCD SPI driver (240×240). design D2.
//!
//! NOTE: change 02 stubs the SPI + ST7789 init sequence. Real SPI driver
//! construction (SpiDriver on SPI2_HOST @ 40MHz, SCLK=GPIO1/MOSI=GPIO2/
//! DC=GPIO3/RST=GPIO4/CS=GPIO11) and the ST7789 register init sequence
//! (sleep out / pixel format / rotation / display on) are added in change
//! 04 (DisplayService) when the slint platform backend is wired up and
//! frame-buffer presentation can be verified on-device.

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct LcdDriver {
    _w: u32,
    _h: u32,
}

impl LcdDriver {
    pub fn init() -> Result<Self, HalError> {
        Ok(Self {
            _w: BoardProfile::LCD_W,
            _h: BoardProfile::LCD_H,
        })
    }

    /// Push a full-frame buffer to the LCD. Stubbed — real SPI DMA push in
    /// change 04 when the slint software renderer feeds it.
    pub fn present(&mut self, _fb: &[u8]) -> Result<(), HalError> {
        Ok(())
    }
}
