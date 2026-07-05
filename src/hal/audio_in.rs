//! ES7210 microphone capture driver: I2C addr 0x40 + I2S0 RX. design D7.
//!
//! NOTE: change 02 stubs I2C + I2S0 RX. Real ES7210 register sequence
//! (power up / 16kHz sample rate / unmute) and I2S0 RX TDM channel
//! configuration (MCLK=GPIO19/BCLK=GPIO20/WS=GPIO22/DIN=GPIO21, 16kHz /
//! 16bit / DMA buffer 6 desc × 240 frames per Waveshare example) are added
//! in change 05 (AudioService) when business audio capture is wired up.
//! Spec: init() SHALL NOT read business audio data (DMA idle).

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct AudioInDriver {
    _i2c_addr: u8,
    _sample_rate: u32,
}

impl AudioInDriver {
    pub fn init() -> Result<Self, HalError> {
        // Verify pin constants are referenced (spec: 引脚常量集中).
        let _ = (
            BoardProfile::I2S_MCLK_PIN,
            BoardProfile::I2S_BCLK_PIN,
            BoardProfile::I2S_WS_PIN,
            BoardProfile::I2S_DIN_PIN,
            BoardProfile::ES7210_I2C_ADDR,
            BoardProfile::OPUS_SAMPLE_RATE,
        );
        Ok(Self {
            _i2c_addr: BoardProfile::ES7210_I2C_ADDR,
            _sample_rate: BoardProfile::OPUS_SAMPLE_RATE,
        })
    }
}
