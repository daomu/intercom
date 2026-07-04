//! Compile-time board profile for the Waveshare ESP32-C6-Touch-LCD-1.54.
//!
//! All hardware constants live here as `pub const` items on a zero-sized
//! `BoardProfile` struct — no runtime instantiation, no generics.
//! Reference: 技术设计 §2.

#![allow(dead_code)]

/// Zero-sized marker exposing all board-level compile-time constants.
pub struct BoardProfile;

impl BoardProfile {
    // ---- LCD (ST7789, SPI) ----
    pub const LCD_W: u32 = 240;
    pub const LCD_H: u32 = 240;

    // ---- Backlight ----
    pub const BACKLIGHT_PWM_SUPPORTED: bool = true;
    pub const DEFAULT_BRIGHTNESS: u8 = 80;
    pub const DEFAULT_SCREEN_OFF_SEC: u32 = 30;

    // ---- Audio channels ----
    pub const MIC_CHANNELS: u8 = 1;
    pub const SPEAKER_CHANNELS: u8 = 1;

    // ---- Power ----
    /// Waveshare 1.54 board has no true power-off FET; standby via screen-off + light sleep.
    pub const SUPPORTS_TRUE_POWER_OFF: bool = false;

    // ---- Pin assignments (match Waveshare ESP32-C6-Touch-LCD-1.54 schematic) ----
    pub const BAT_ADC_PIN: u8 = 0; // ×3 divider
    pub const PA_CTRL_PIN: u8 = 15; // NS4150B PA shutdown
    pub const BACKLIGHT_PIN: u8 = 6;
    pub const BOOT_BTN_PIN: u8 = 9; // doubles as PTT
    pub const PLUS_BTN_PIN: u8 = 18; // volume / settings
    pub const PWR_BTN_PIN: u8 = 7; // ESP32-C6 EN/bootloader-handled long press

    // ---- Group / RF ----
    pub const MAX_GROUP_SIZE: u8 = 4;
    pub const DISCOVERY_CHANNEL: u8 = 1; // initial pairing channel

    // ---- Opus codec ----
    pub const OPUS_SAMPLE_RATE: u32 = 16000;
    pub const OPUS_FRAME_MS: u32 = 20;

    // ---- Jitter buffer ----
    pub const JITTER_INIT_FRAMES: u8 = 3; // 60ms initial water level

    // ---- Battery ADC ----
    /// External resistor divider ratio on BAT_ADC (R1:R2 = 2:1 → ×3).
    pub const BAT_ADC_DIVIDER: u32 = 3;
}
