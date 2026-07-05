//! Battery ADC driver: ADC1 CH0 = GPIO0 (×3 divider). design D6.
//!
//! NOTE: change 02 stubs the ADC handle. Real AdcDriver on ADC1_CH0 with
//! 12-bit sampling is added in change 04 (PowerService) when EWMA smoothing
//! and 4-level hysteresis mapping are wired up. Spec D4 / D6: BSP layer
//! provides only `read_raw_adc() -> u16`; no `smoothed()` / `step()`.

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct BatteryDriver {
    _pin: u8,
}

impl BatteryDriver {
    pub fn init() -> Result<Self, HalError> {
        Ok(Self {
            _pin: BoardProfile::BAT_ADC_PIN,
        })
    }

    /// Single 12-bit raw ADC sample. Stubbed — returns 0 until change 04
    /// wires the AdcDriver. Real range: 1130-1580 for 3.0-4.2V cell.
    pub fn read_raw_adc(&mut self) -> Result<u16, HalError> {
        Ok(0)
    }
}
