//! Hardware abstraction layer: BSP drivers for ST7789/CST816/ES7210/ES8311/ADC/buttons.
//! Aggregator that initializes all peripherals. design D10.
//!
//! Real peripheral bindings: `Peripherals::take()` is called once here and
//! peripheral ownership is split among the 8 sub-drivers. The shared I2C bus
//! (touch + ES8311 + ES7210) and shared I2S0 driver (audio in + out) are
//! stored as public fields on `Hal` and passed by `&mut` reference to device
//! methods at call time (split-borrow pattern).

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

use esp_idf_svc::hal::i2c::{config::Config as I2cConfig, I2cDriver};
use esp_idf_svc::hal::i2s::{config::StdConfig, I2sBiDir, I2sDriver};
use esp_idf_svc::hal::peripherals::Peripherals;

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
/// ESP32-C6, no Sync required). The shared I2C + I2S buses are public fields
/// so service layers can split-borrow them into device methods.
pub struct Hal {
    pub lcd: lcd::LcdDriver,
    pub touch: touch::TouchDriver,
    pub buttons: buttons::ButtonsDriver,
    pub backlight: backlight::BacklightDriver,
    pub battery: battery::BatteryDriver,
    pub audio_in: audio_in::AudioInDriver,
    pub audio_out: audio_out::AudioOutDriver,
    pub radio: radio::RadioDriver,
    /// Shared I2C bus (touch + ES8311 + ES7210 all at different addresses).
    pub i2c: I2cDriver<'static>,
    /// Shared I2S0 driver (full-duplex bidir — TX to ES8311, RX from ES7210).
    pub i2s: I2sDriver<'static, I2sBiDir>,
}

/// Initialize all peripherals in fixed order (design D10). On any failure,
/// returns `HalError` immediately and stops further init.
pub fn init() -> Result<Hal, HalError> {
    // Take the peripheral singleton once; ownership is split among drivers.
    let peripherals = Peripherals::take()
        .map_err(|e| HalError::BacklightInitFailed(format!("Peripherals::take: {e}")))?;

    // 1. Backlight (first, so the panel is lit during subsequent init).
    let backlight = backlight::BacklightDriver::init(
        peripherals.ledc.timer0,
        peripherals.ledc.channel0,
        peripherals.pins.gpio6,
    )?;
    log::info!("Backlight init OK");

    // 2. LCD (SPI2 + ST7789 reset sequence).
    let lcd = lcd::LcdDriver::init(
        peripherals.spi2,
        peripherals.pins.gpio1, // SCLK
        peripherals.pins.gpio2, // MOSI
        peripherals.pins.gpio3, // DC
        peripherals.pins.gpio4, // RST
        peripherals.pins.gpio11, // CS
    )?;
    log::info!("Lcd init OK");

    // 3. Shared I2C bus (SDA=GPIO8, SCL=GPIO7) for touch + ES8311 + ES7210.
    let i2c_config = I2cConfig::new().baudrate(esp_idf_svc::hal::units::Hertz(400_000));
    let mut i2c = I2cDriver::new(
        peripherals.i2c0,
        peripherals.pins.gpio8, // SDA
        peripherals.pins.gpio7, // SCL
        &i2c_config,
    )
    .map_err(|e| HalError::TouchInitFailed(format!("I2C bus: {e}")))?;
    log::info!("I2C bus init OK");

    // 4. Touch (CST816 on shared I2C bus, addr 0x15).
    let touch = touch::TouchDriver::init(&mut i2c)?;
    log::info!("Touch init OK");

    // 5. Buttons (BOOT=GPIO9 + PLUS=GPIO18, input + pull-up).
    let buttons = buttons::ButtonsDriver::init(
        peripherals.pins.gpio9,  // BOOT
        peripherals.pins.gpio18, // PLUS
    )?;
    log::info!("Buttons init OK (BOOT+PLUS; PWR GPIO unavailable on this board)");

    // 6. Battery ADC (ADC1 CH0 = GPIO0).
    let battery = battery::BatteryDriver::init(peripherals.adc1, peripherals.pins.gpio0)?;
    log::info!("Battery init OK");

    // 7. Shared I2S0 driver (full-duplex: MCLK=GPIO19, BCLK=GPIO20,
    //    WS=GPIO22, DOUT=GPIO23→ES8311, DIN=GPIO21←ES7210).
    let i2s_config = StdConfig::philips(
        BoardProfile::OPUS_SAMPLE_RATE,
        esp_idf_svc::hal::i2s::config::DataBitWidth::Bits16,
    );
    let i2s = I2sDriver::new_std_bidir(
        peripherals.i2s0,
        &i2s_config,
        peripherals.pins.gpio20, // BCLK
        peripherals.pins.gpio21, // DIN (← ES7210)
        peripherals.pins.gpio23, // DOUT (→ ES8311)
        Some(peripherals.pins.gpio19), // MCLK
        peripherals.pins.gpio22, // WS
    )
    .map_err(|e| HalError::AudioInInitFailed(format!("I2S bus: {e}")))?;
    log::info!("I2S bus init OK");

    // 8. AudioIn (ES7210 on shared I2C addr 0x40; capture via shared I2S0).
    let audio_in = audio_in::AudioInDriver::init()?;
    log::info!("AudioIn init OK");

    // 9. AudioOut (ES8311 on shared I2C addr 0x18 + PA_CTRL GPIO15).
    let audio_out = audio_out::AudioOutDriver::init(peripherals.pins.gpio15)?;
    log::info!("AudioOut init OK");

    // 10. Radio (EspWifi STA + EspNow on DISCOVERY_CHANNEL).
    // Note: WiFi init may briefly disrupt USB CDC — monitor may disconnect.
    log::info!("Radio init starting (WiFi + ESP-NOW)...");
    let radio = radio::RadioDriver::init(peripherals.modem)?;
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
        i2c,
        i2s,
    })
}

// Compile-time Send assertion (spec: 驱动句柄 Send). Real peripheral handles
// from esp-idf-hal/svc are all `unsafe impl Send`; the Hal struct is Send
// iff every field is Send.
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
    assert_send::<I2cDriver<'static>>();
    assert_send::<I2sDriver<'static, I2sBiDir>>();
};
