//! ES8311 + NS4150B playback driver: I2C addr 0x18 + I2S0 TX + PA_CTRL GPIO15. design D8.
//!
//! Real wiring: PA_CTRL (GPIO15) is a PinDriver<Output> for direct GPIO
//! toggle. The ES8311 codec is configured via the shared I2C bus (owned by
//! `Hal`, passed as `&mut I2cDriver` to `init_codec()`). The I2S0 driver
//! (full-duplex bidir, owned by `Hal`) is passed as `&mut I2sDriver` to
//! `write_pcm()`.
//!
//! Spec D6: `pa_enable(on)` is a pure GPIO toggle — no delay / soft startup
//! (that's AudioService's job).
//!
//! ES8311 register init is a minimal power-up sequence; on-device refinement
//! needed for exact clock/volume config. See Waveshare bsp_es8311.c.

#![allow(dead_code)]

use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::gpio::{Output, OutputPin, PinDriver};
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{I2sBiDir, I2sDriver};

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct AudioOutDriver {
    i2c_addr: u8,
    pa: PinDriver<'static, Output>,
}

impl std::fmt::Debug for AudioOutDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioOutDriver")
            .field("i2c_addr", &self.i2c_addr)
            .field("pa_pin", &BoardProfile::PA_CTRL_PIN)
            .finish_non_exhaustive()
    }
}

impl AudioOutDriver {
    /// Construct from owned PA_CTRL GPIO pin. The ES8311 codec is configured
    /// separately via `init_codec()` using the shared I2C bus.
    pub fn init<PAPIN>(pa_pin: PAPIN) -> Result<Self, HalError>
    where
        PAPIN: OutputPin + 'static,
    {
        let _ = (
            BoardProfile::I2S_MCLK_PIN,
            BoardProfile::I2S_BCLK_PIN,
            BoardProfile::I2S_WS_PIN,
            BoardProfile::I2S_DOUT_PIN,
        );
        let mut pa = PinDriver::output(pa_pin)
            .map_err(|e| HalError::AudioOutInitFailed(format!("PA_CTRL pin: {e}")))?;
        // PA off at boot (active-low NS4150B — low = shutdown).
        pa.set_low()
            .map_err(|e| HalError::AudioOutInitFailed(format!("PA low: {e}")))?;
        Ok(Self {
            i2c_addr: BoardProfile::ES8311_I2C_ADDR,
            pa,
        })
    }

    /// Pure GPIO toggle: on=true → high, on=false → low. No delay.
    pub fn pa_enable(&mut self, on: bool) -> Result<(), HalError> {
        if on {
            self.pa
                .set_high()
                .map_err(|e| HalError::AudioOutInitFailed(format!("PA high: {e}")))
        } else {
            self.pa
                .set_low()
                .map_err(|e| HalError::AudioOutInitFailed(format!("PA low: {e}")))
        }
    }

    /// Configure the ES8311 codec via the shared I2C bus. Minimal power-up
    /// sequence: reset → power up → DAC enable → unmute. On-device refinement
    /// needed for exact clock/volume settings.
    pub fn init_codec(&self, i2c: &mut I2cDriver<'static>) -> Result<(), HalError> {
        let addr = self.i2c_addr;
        let mut write = |reg: u8, val: u8| -> Result<(), HalError> {
            i2c.write(addr, &[reg, val], BLOCK)
                .map_err(|e| HalError::AudioOutInitFailed(format!("ES8311 write 0x{reg:02X}: {e}")))
        };
        write(0x00, 0x80)?; // Reset
        write(0x01, 0x3F)?; // Power up all blocks
        write(0x02, 0x00)?; // Clock manager: power up
        write(0x03, 0x10)?; // Clock manager: set MCLK source
        write(0x04, 0x80)?; // Clock manager: Set clock divider
        write(0x05, 0x00)?; // Clock manager
        write(0x06, 0x03)?; // Clock manager: BCLK enable
        write(0x07, 0x07)?; // Clock manager
        write(0x08, 0xFF)?; // Clock manager
        write(0x09, 0x0C)?; // Clock manager
        write(0x0A, 0x00)?; // Clock manager
        write(0x0B, 0x00)?; // Clock manager
        write(0x0C, 0x00)?; // Clock manager
        write(0x14, 0x1A)?; // DAC: power up
        write(0x1B, 0x00)?; // DAC: unmute
        write(0x1C, 0x02)?; // DAC volume high
        write(0x1D, 0x00)?; // DAC volume low
        write(0x1E, 0x00)?; // DAC
        write(0x1F, 0x00)?; // DAC
        write(0x2B, 0x00)?; // System: power up ADC/DAC
        write(0x2C, 0x00)?; // System
        write(0x2D, 0x00)?; // System
        write(0x2E, 0x00)?; // ADC config
        write(0x2F, 0x00)?; // ADC
        Ok(())
    }

    /// Write PCM audio data to the I2S0 TX channel. `buf` is interleaved
    /// 16-bit samples. The I2S driver is shared (owned by `Hal`).
    pub fn write_pcm(&mut self, i2s: &mut I2sDriver<'static, I2sBiDir>, buf: &[u8]) -> Result<(), HalError> {
        i2s.write_all(buf, BLOCK)
            .map_err(|e| HalError::AudioOutInitFailed(format!("I2S write: {e}")))
    }
}
