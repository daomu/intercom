//! Battery ADC driver: ADC1 CH0 = GPIO0 (×3 divider). design D6.
//!
//! Real ADC wiring: oneshot::AdcDriver on ADC1 + AdcChannelDriver on
//! ADC1_CH0 (GPIO0). 12-bit resolution, 12dB attenuation for 0-~3.1V range.
//! The AdcDriver is moved into the AdcChannelDriver (which owns it via
//! `Borrow`), so only the channel driver is stored.
//!
//! Spec D4/D6: BSP layer provides only `read_raw_adc() -> u16`. EWMA
//! smoothing + 4-level hysteresis mapping live in PowerService.

#![allow(dead_code)]

use esp_idf_svc::hal::adc::{
    attenuation::DB_12, oneshot::AdcChannelDriver, oneshot::AdcDriver, ADCCH0, ADCU1, Resolution,
};
use esp_idf_svc::hal::gpio::ADCPin;

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct BatteryDriver {
    channel: AdcChannelDriver<'static, ADCCH0<ADCU1>, AdcDriver<'static, ADCU1>>,
}

impl std::fmt::Debug for BatteryDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatteryDriver")
            .field("pin", &BoardProfile::BAT_ADC_PIN)
            .finish_non_exhaustive()
    }
}

impl BatteryDriver {
    /// Construct from owned ADC1 peripheral + GPIO0 (ADC1_CH0) pin.
    pub fn init<ADC, PIN>(adc: ADC, pin: PIN) -> Result<Self, HalError>
    where
        ADC: esp_idf_svc::hal::adc::Adc<AdcUnit = ADCU1> + 'static,
        PIN: ADCPin<AdcChannel = ADCCH0<ADCU1>> + 'static,
    {
        let adc_driver = AdcDriver::new(adc)
            .map_err(|e| HalError::BatteryInitFailed(format!("ADC driver: {e}")))?;
        let config = esp_idf_svc::hal::adc::oneshot::config::AdcChannelConfig {
            attenuation: DB_12,
            resolution: Resolution::Resolution12Bit,
            calibration: esp_idf_svc::hal::adc::oneshot::config::Calibration::None,
        };
        let channel = AdcChannelDriver::new(adc_driver, pin, &config)
            .map_err(|e| HalError::BatteryInitFailed(format!("ADC channel: {e}")))?;
        Ok(Self { channel })
    }

    /// Single 12-bit raw ADC sample (0..=4095). Real range: 1130-1580 for
    /// 3.0-4.2V cell through ×3 divider.
    pub fn read_raw_adc(&mut self) -> Result<u16, HalError> {
        self.channel
            .read_raw()
            .map_err(|e| HalError::BatteryInitFailed(format!("ADC read: {e}")))
    }
}
