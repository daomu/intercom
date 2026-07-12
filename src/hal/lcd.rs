//! ST7789 LCD SPI driver (240×240). design D2.
//!
//! Real SPI wiring: SpiDriver on SPI2_HOST @ 40MHz (SCLK=GPIO1, MOSI=GPIO2,
//! CS=GPIO11), SpiDeviceDriver wrapping it. DC=GPIO3 and RST=GPIO4 are
//! PinDriver<Output> toggled manually for command/data mode switching.
//!
//! ST7789 init sequence: software reset → sleep out → pixel format (RGB565)
//! → memory access control (rotation) → column/row address set → display on.
//! `present()` sets the full-frame address window then pushes the framebuffer.
//!
//! NB: manual DC toggling between separate SPI writes has a small timing gap
//! (non-atomic). This is the standard esp-idf-hal pattern without the
//! `esp_lcd` peripheral's hardware DC management. On-device verification will
//! confirm whether the gap causes visible artifacts; if so, switching to
//! `esp_lcd_panel_io_spi` is the fix.

#![allow(dead_code)]

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{Output, OutputPin, PinDriver};
use esp_idf_svc::hal::spi::{
    config::{Config, DriverConfig},
    SpiDeviceDriver, SpiDriver,
};
use esp_idf_svc::hal::units::Hertz;

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct LcdDriver {
    spi: SpiDeviceDriver<'static, SpiDriver<'static>>,
    dc: PinDriver<'static, Output>,
    _rst: PinDriver<'static, Output>,
    w: u32,
    h: u32,
}

impl std::fmt::Debug for LcdDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LcdDriver")
            .field("w", &self.w)
            .field("h", &self.h)
            .finish_non_exhaustive()
    }
}

impl LcdDriver {
    /// Construct from owned SPI2 peripheral + SCLK/MOSI/DC/RST/CS pins.
    pub fn init<SPI, SCLK, MOSI, DCPIN, RSTPIN, CSPIN>(
        spi: SPI,
        sclk: SCLK,
        mosi: MOSI,
        dc_pin: DCPIN,
        rst_pin: RSTPIN,
        cs_pin: CSPIN,
    ) -> Result<Self, HalError>
    where
        SPI: esp_idf_svc::hal::spi::SpiAnyPins + 'static,
        SCLK: OutputPin + 'static,
        MOSI: OutputPin + 'static,
        DCPIN: OutputPin + 'static,
        RSTPIN: OutputPin + 'static,
        CSPIN: OutputPin + 'static,
    {
        // ESP32-C6 only supports SPI DMA auto-alloc (no manual Channel1/2).
        let bus_config = DriverConfig::new().dma(esp_idf_svc::hal::spi::Dma::Auto(4096));
        let spi_driver = SpiDriver::new(spi, sclk, mosi, None::<esp_idf_svc::hal::gpio::AnyInputPin>, &bus_config)
            .map_err(|e| HalError::LcdInitFailed(format!("SPI driver: {e}")))?;
        let dev_config = Config::new()
            .baudrate(Hertz(BoardProfile::LCD_SPI_FREQ_HZ))
            .write_only(true);
        let spi_dev = SpiDeviceDriver::new(spi_driver, Some(cs_pin), &dev_config)
            .map_err(|e| HalError::LcdInitFailed(format!("SPI device: {e}")))?;

        let dc = PinDriver::output(dc_pin)
            .map_err(|e| HalError::LcdInitFailed(format!("DC pin: {e}")))?;
        let mut rst = PinDriver::output(rst_pin)
            .map_err(|e| HalError::LcdInitFailed(format!("RST pin: {e}")))?;

        // Hardware reset: RST low 10ms, high 10ms.
        rst.set_low().map_err(|e| HalError::LcdInitFailed(format!("RST low: {e}")))?;
        FreeRtos::delay_ms(10);
        rst.set_high().map_err(|e| HalError::LcdInitFailed(format!("RST high: {e}")))?;
        FreeRtos::delay_ms(10);

        let mut drv = Self {
            spi: spi_dev,
            dc,
            _rst: rst,
            w: BoardProfile::LCD_W,
            h: BoardProfile::LCD_H,
        };

        // ST7789 init sequence.
        drv.write_cmd(0x01, &[])?; // Software reset
        FreeRtos::delay_ms(120);
        drv.write_cmd(0x11, &[])?; // Sleep out
        FreeRtos::delay_ms(120);
        drv.write_cmd(0x3A, &[0x55])?; // 16-bit RGB565 pixel format
        drv.write_cmd(0x36, &[0x00])?; // Memory access control (normal orientation)
        // Column address set: 0..240
        drv.write_cmd(0x2A, &[0x00, 0x00, 0x00, 0xEF])?;
        // Row address set: 0..240
        drv.write_cmd(0x2B, &[0x00, 0x00, 0x00, 0xEF])?;
        drv.write_cmd(0x29, &[])?; // Display on
        FreeRtos::delay_ms(50);

        Ok(drv)
    }

    /// Write a command byte (DC low) followed by data bytes (DC high).
    fn write_cmd(&mut self, cmd: u8, data: &[u8]) -> Result<(), HalError> {
        self.dc
            .set_low()
            .map_err(|e| HalError::LcdInitFailed(format!("DC low: {e}")))?;
        self.spi
            .write(&[cmd])
            .map_err(|e| HalError::LcdInitFailed(format!("SPI cmd write: {e}")))?;
        if !data.is_empty() {
            self.dc
                .set_high()
                .map_err(|e| HalError::LcdInitFailed(format!("DC high: {e}")))?;
            self.spi
                .write(data)
                .map_err(|e| HalError::LcdInitFailed(format!("SPI data write: {e}")))?;
        }
        Ok(())
    }

    /// Push a full-frame RGB565 buffer to the LCD (240×240×2 = 115,200 bytes).
    /// Sets the address window to full screen, then writes the framebuffer
    /// with DC high (data mode).
    pub fn present(&mut self, fb: &[u8]) -> Result<(), HalError> {
        // Set full-screen column address window.
        self.write_cmd(0x2A, &[
            0x00,
            0x00,
            ((self.w - 1) >> 8) as u8,
            ((self.w - 1) & 0xFF) as u8,
        ])?;
        // Set full-screen row address window.
        self.write_cmd(0x2B, &[
            0x00,
            0x00,
            ((self.h - 1) >> 8) as u8,
            ((self.h - 1) & 0xFF) as u8,
        ])?;
        // RAMWR (0x2C) — tell ST7789 the following bytes are pixel data
        // written to GRAM. Without this command the pixel bytes would be
        // misinterpreted as params of the previous command (0x2B/RASET) and
        // GRAM would never be written → garbage screen.
        self.write_cmd(0x2C, &[])?;
        // DC high for the framebuffer data (write_cmd(0x2C,&[]) leaves DC low).
        self.dc
            .set_high()
            .map_err(|e| HalError::LcdInitFailed(format!("DC high: {e}")))?;
        // Chunk the framebuffer to respect SPI max transfer size (4KB typical).
        for chunk in fb.chunks(4096) {
            self.spi
                .write(chunk)
                .map_err(|e| HalError::LcdInitFailed(format!("SPI fb write: {e}")))?;
        }
        Ok(())
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (self.w, self.h)
    }
}
