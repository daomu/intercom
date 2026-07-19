//! Backlight LEDC PWM driver for GPIO6 (BL pin on Waveshare 1.54 board).
//! 5kHz / 8-bit duty. design D5.
//!
//! Real LEDC PWM wiring: LedcTimerDriver on LEDC timer0 (low-speed),
//! LedcDriver on channel0, output to GPIO6. Brightness 0..=100 maps to
//! duty 0..=255. The `LedcTimerDriver` is kept alive as a field because its
//! Drop impl resets the timer (which would stop PWM).

#![allow(dead_code)]

use esp_idf_svc::hal::gpio::OutputPin;
use esp_idf_svc::hal::ledc::{
    config::TimerConfig, LedcChannel, LedcDriver, LedcTimer, LedcTimerDriver, LowSpeed, Resolution,
};
use esp_idf_svc::hal::units::Hertz;

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct BacklightDriver {
    _timer: LedcTimerDriver<'static, LowSpeed>,
    channel: LedcDriver<'static>,
    last_nonzero_pct: u8,
}

impl std::fmt::Debug for BacklightDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BacklightDriver")
            .field("last_nonzero_pct", &self.last_nonzero_pct)
            .finish_non_exhaustive()
    }
}

impl BacklightDriver {
    /// Construct from owned LEDC timer/channel + backlight GPIO pin.
    pub fn init<TIMER, CHANNEL, PIN>(
        timer: TIMER,
        channel: CHANNEL,
        pin: PIN,
    ) -> Result<Self, HalError>
    where
        TIMER: LedcTimer<SpeedMode = LowSpeed> + 'static,
        CHANNEL: LedcChannel<SpeedMode = LowSpeed> + 'static,
        PIN: OutputPin + 'static,
    {
        let timer_cfg = TimerConfig::new()
            .frequency(Hertz(BoardProfile::BACKLIGHT_PWM_FREQ_HZ))
            .resolution(Resolution::Bits8);
        let timer_driver = LedcTimerDriver::new(timer, &timer_cfg)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC timer: {e}")))?;
        let channel_driver = LedcDriver::new(channel, &timer_driver, pin)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC channel: {e}")))?;

        let mut drv = Self {
            _timer: timer_driver,
            channel: channel_driver,
            last_nonzero_pct: BoardProfile::DEFAULT_BRIGHTNESS,
        };
        // Apply default brightness on boot.
        let duty = Self::pct_to_duty(drv.last_nonzero_pct);
        drv.channel
            .set_duty(duty)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC set_duty: {e}")))?;
        Ok(drv)
    }

    /// Minimum applied duty so brightness 0 does not fully black out the
    /// screen (wire-settings-side-effects: floor at ~5% of the 8-bit range so
    /// the UI keeps visual feedback). `off()` bypasses this to reach duty 0.
    const MIN_VISIBLE_DUTY: u32 = 13; // ≈5% of 255

    fn pct_to_duty(pct: u8) -> u32 {
        let pct = pct.min(100) as u32;
        // 8-bit resolution → max duty = 255. Map 0..=100 → 0..=255.
        (pct * 255) / 100
    }

    /// Set brightness 0..=100. Persists non-zero value for `on()` recovery.
    /// The applied duty is floored at `MIN_VISIBLE_DUTY` so a stored value of
    /// 0 still leaves the panel dimly lit (never a full black-out).
    pub fn set_brightness(&mut self, v: u8) -> Result<(), HalError> {
        let v = v.min(100);
        if v != 0 {
            self.last_nonzero_pct = v;
        }
        let duty = Self::pct_to_duty(v).max(Self::MIN_VISIBLE_DUTY);
        self.channel
            .set_duty(duty)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC set_duty: {e}")))
    }

    /// Backlight off (duty = 0).
    pub fn off(&mut self) -> Result<(), HalError> {
        self.channel
            .set_duty(0)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC set_duty: {e}")))
    }

    /// Restore last non-zero brightness (or DEFAULT_BRIGHTNESS).
    pub fn on(&mut self) -> Result<(), HalError> {
        let duty = Self::pct_to_duty(self.last_nonzero_pct);
        self.channel
            .set_duty(duty)
            .map_err(|e| HalError::BacklightInitFailed(format!("LEDC set_duty: {e}")))
    }

    pub fn last_nonzero_pct(&self) -> u8 {
        self.last_nonzero_pct
    }
}
