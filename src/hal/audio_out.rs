//! ES8311 + NS4150B playback driver: I2C addr 0x18 + I2S0 TX + PA_CTRL GPIO15. design D8.
//!
//! NOTE: change 02 stubs I2C + I2S0 TX + PA_CTRL. Real ES8311 register
//! sequence + I2S0 TX STD channel (MCLK=GPIO19/BCLK=GPIO20/WS=GPIO22/
//! DOUT=GPIO23, 16kHz / 16bit / DMA buffer) + PA_CTRL as output low are
//! added in change 05 (AudioService) when business audio playback is wired
//! up. Spec D6: `pa_enable(on)` is a pure GPIO toggle — no delay / soft
//! startup (that's change 05 AudioService's job).

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct AudioOutDriver {
    _i2c_addr: u8,
    _pa_pin: u8,
}

impl AudioOutDriver {
    pub fn init() -> Result<Self, HalError> {
        // Verify pin constants are referenced (spec: 引脚常量集中).
        let _ = (
            BoardProfile::I2S_MCLK_PIN,
            BoardProfile::I2S_BCLK_PIN,
            BoardProfile::I2S_WS_PIN,
            BoardProfile::I2S_DOUT_PIN,
            BoardProfile::ES8311_I2C_ADDR,
            BoardProfile::PA_CTRL_PIN,
        );
        Ok(Self {
            _i2c_addr: BoardProfile::ES8311_I2C_ADDR,
            _pa_pin: BoardProfile::PA_CTRL_PIN,
        })
    }

    /// Pure GPIO toggle: on=true → high, on=false → low. No delay.
    /// Stubbed — real GPIO write in change 05.
    pub fn pa_enable(&mut self, _on: bool) -> Result<(), HalError> {
        Ok(())
    }
}
