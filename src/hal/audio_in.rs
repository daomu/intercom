//! ES7210 microphone capture driver: I2C addr 0x40 + I2S0 RX. design D7.
//!
//! Real wiring: the ES7210 codec is configured via the shared I2C bus (owned
//! by `Hal`, passed as `&mut I2cDriver` to `init_codec()`). The I2S0 driver
//! (full-duplex bidir, owned by `Hal`) is passed as `&mut I2sDriver` to
//! `read_pcm()`.
//!
//! Spec: init() SHALL NOT read business audio data (DMA idle). AudioService
//! starts capture after arming.
//!
//! ES7210 register init is a minimal power-up sequence; on-device refinement
//! needed for exact TDM channel config. See Waveshare bsp_es7210.c.

#![allow(dead_code)]

use esp_idf_svc::hal::delay::BLOCK;
use esp_idf_svc::hal::i2c::I2cDriver;
use esp_idf_svc::hal::i2s::{I2sBiDir, I2sDriver};

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct AudioInDriver {
    i2c_addr: u8,
    sample_rate: u32,
}

impl std::fmt::Debug for AudioInDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioInDriver")
            .field("i2c_addr", &self.i2c_addr)
            .field("sample_rate", &self.sample_rate)
            .finish_non_exhaustive()
    }
}

impl AudioInDriver {
    /// Construct with the ES7210 I2C address + sample rate. The codec is
    /// configured via `init_codec()` using the shared I2C bus.
    pub fn init() -> Result<Self, HalError> {
        let _ = (
            BoardProfile::I2S_MCLK_PIN,
            BoardProfile::I2S_BCLK_PIN,
            BoardProfile::I2S_WS_PIN,
            BoardProfile::I2S_DIN_PIN,
        );
        Ok(Self {
            i2c_addr: BoardProfile::ES7210_I2C_ADDR,
            sample_rate: BoardProfile::OPUS_SAMPLE_RATE,
        })
    }

    /// Configure the ES7210 codec via the shared I2C bus. Minimal power-up
    /// sequence: reset → power up → unmute. On-device refinement needed for
    /// exact TDM/channel/gain settings.
    pub fn init_codec(&self, i2c: &mut I2cDriver<'static>) -> Result<(), HalError> {
        let addr = self.i2c_addr;
        let mut write = |reg: u8, val: u8| -> Result<(), HalError> {
            i2c.write(addr, &[reg, val], BLOCK)
                .map_err(|e| HalError::AudioInInitFailed(format!("ES7210 write 0x{reg:02X}: {e}")))
        };
        // ES7210 register sequence (from common ESP32 codec drivers).
        write(0x00, 0xFF)?; // Reset
        write(0x00, 0x32)?; // Power up
        write(0x01, 0x00)?; // Clock manager: power up
        write(0x02, 0x00)?; // Clock manager
        write(0x03, 0x10)?; // Clock manager: set MCLK divider
        write(0x04, 0x40)?; // Clock manager
        write(0x05, 0x00)?; // Clock manager
        write(0x06, 0x03)?; // Clock manager: BCLK enable
        write(0x07, 0x00)?; // Clock manager
        write(0x08, 0x00)?; // Clock manager
        write(0x09, 0x00)?; // Clock manager
        write(0x0A, 0x00)?; // Clock manager
        write(0x0B, 0x00)?; // Clock manager
        write(0x0C, 0x00)?; // Clock manager
        write(0x11, 0x60)?; // ADC: power up + unmute
        write(0x12, 0x00)?; // ADC: gain setting
        write(0x13, 0x20)?; // ADC
        write(0x14, 0x00)?; // ADC
        write(0x21, 0x00)?; // ADC channel config
        write(0x22, 0x00)?; // ADC
        write(0x23, 0x00)?; // ADC
        write(0x24, 0x00)?; // ADC
        write(0x25, 0x00)?; // ADC
        Ok(())
    }

    /// Read PCM audio data from the I2S0 RX channel. `buf` receives
    /// interleaved 16-bit samples. The I2S driver is shared (owned by `Hal`).
    pub fn read_pcm(
        &self,
        i2s: &mut I2sDriver<'static, I2sBiDir>,
        buf: &mut [u8],
    ) -> Result<usize, HalError> {
        i2s.read(buf, BLOCK)
            .map_err(|e| HalError::AudioInInitFailed(format!("I2S read: {e}")))
    }
}
