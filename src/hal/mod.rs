//! Hardware abstraction layer: BSP drivers for ST7789/CST816/ES7210/ES8311/ADC/buttons.
//! Aggregator that initializes all peripherals. design D10.
//!
//! NOTE: change 02 ships type-correct driver stubs that expose the spec-
//! required API surface (`LcdDriver::present`, `TouchDriver::read_event`,
//! `BacklightDriver::set_brightness/on/off`, `BatteryDriver::read_raw_adc`,
//! `AudioOutDriver::pa_enable`, etc.). The actual ESP-IDF peripheral handle
//! bindings (SpiDriver / I2cDriver / I2sDriver / LedcDriver / AdcDriver /
//! EspWifi / EspNow) and register-level init sequences (ST7789 / CST816 /
//! ES7210 / ES8311) are added incrementally in changes 04–06 when the
//! consuming Service layers are wired up and on-device verification is
//! possible. The change 02 acceptance criterion is `cargo build` passing
//! (tasks 12.1–12.2) and `Hal::init` not panicking once peripheral handles
//! are wired up (tasks 13.1–13.5 are on-device hardware verification).

#![allow(dead_code)]

pub mod audio_in;
pub mod audio_out;
pub mod backlight;
pub mod battery;
pub mod buttons;
pub mod lcd;
pub mod radio;
pub mod touch;

use std::fmt;

use crate::board_profile::BoardProfile;

/// Error type for any BSP init failure. Each variant carries a String context.
#[derive(Debug)]
pub enum HalError {
    BacklightInitFailed(String),
    LcdInitFailed(String),
    TouchInitFailed(String),
    ButtonsInitFailed(String),
    BatteryInitFailed(String),
    AudioInInitFailed(String),
    AudioOutInitFailed(String),
    RadioInitFailed(String),
}

impl fmt::Display for HalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HalError::BacklightInitFailed(s) => write!(f, "Backlight init failed: {s}"),
            HalError::LcdInitFailed(s) => write!(f, "Lcd init failed: {s}"),
            HalError::TouchInitFailed(s) => write!(f, "Touch init failed: {s}"),
            HalError::ButtonsInitFailed(s) => write!(f, "Buttons init failed: {s}"),
            HalError::BatteryInitFailed(s) => write!(f, "Battery init failed: {s}"),
            HalError::AudioInInitFailed(s) => write!(f, "AudioIn init failed: {s}"),
            HalError::AudioOutInitFailed(s) => write!(f, "AudioOut init failed: {s}"),
            HalError::RadioInitFailed(s) => write!(f, "Radio init failed: {s}"),
        }
    }
}

impl std::error::Error for HalError {}

/// Aggregated BSP handle holding every peripheral driver. Send (single-core
/// ESP32-C6, no Sync required). All sub-drivers are stubs without peripheral
/// handles in change 02; real handles added in changes 04–06.
pub struct Hal {
    pub lcd: lcd::LcdDriver,
    pub touch: touch::TouchDriver,
    pub buttons: buttons::ButtonsDriver,
    pub backlight: backlight::BacklightDriver,
    pub battery: battery::BatteryDriver,
    pub audio_in: audio_in::AudioInDriver,
    pub audio_out: audio_out::AudioOutDriver,
    pub radio: radio::RadioDriver,
}

/// Initialize all peripherals in fixed order (design D10). On any failure,
/// returns `HalError` immediately and stops further init.
pub fn init() -> Result<Hal, HalError> {
    // 1. Backlight (first, so the panel is lit during subsequent init).
    let backlight = backlight::BacklightDriver::init()?;
    log::info!("Backlight init OK");

    // 2. LCD (SPI + ST7789 reset sequence).
    let lcd = lcd::LcdDriver::init()?;
    log::info!("Lcd init OK");

    // 3. Touch (I2C + CST816 + IRQ).
    let touch = touch::TouchDriver::init()?;
    log::info!("Touch init OK");

    // 4. Buttons (BOOT + PLUS GPIO edge interrupts; PWR unavailable on this board).
    let buttons = buttons::ButtonsDriver::init()?;
    log::info!("Buttons init OK (BOOT+PLUS; PWR GPIO unavailable on this board)");

    // 5. Battery ADC (ADC1 CH0 = GPIO0).
    let battery = battery::BatteryDriver::init()?;
    log::info!("Battery init OK");

    // 6. AudioIn (ES7210 I2C + I2S0 RX TDM).
    let audio_in = audio_in::AudioInDriver::init()?;
    log::info!("AudioIn init OK");

    // 7. AudioOut (ES8311 I2C + I2S0 TX STD + PA_CTRL low).
    let audio_out = audio_out::AudioOutDriver::init()?;
    log::info!("AudioOut init OK");

    // 8. Radio (EspWifi STA + EspNow channel=DISCOVERY_CHANNEL).
    let radio = radio::RadioDriver::init()?;
    log::info!("Radio init OK");

    // Verify BoardProfile references compile (spec: 引脚常量集中).
    let _ = (
        BoardProfile::LCD_W,
        BoardProfile::LCD_H,
        BoardProfile::DISCOVERY_CHANNEL,
    );

    Ok(Hal {
        lcd,
        touch,
        buttons,
        backlight,
        battery,
        audio_in,
        audio_out,
        radio,
    })
}

// Compile-time Send assertion (spec: 驱动句柄 Send). All stubs are trivially
// Send since they hold only u8/u32 Copy fields. Real peripheral handles added
// in changes 04–06 must preserve Send.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Hal>();
    assert_send::<lcd::LcdDriver>();
    assert_send::<touch::TouchDriver>();
    assert_send::<buttons::ButtonsDriver>();
    assert_send::<backlight::BacklightDriver>();
    assert_send::<battery::BatteryDriver>();
    assert_send::<audio_in::AudioInDriver>();
    assert_send::<audio_out::AudioOutDriver>();
    assert_send::<radio::RadioDriver>();
};
