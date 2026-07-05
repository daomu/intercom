//! Compile-time board profile for the Waveshare ESP32-C6-Touch-LCD-1.54.
//!
//! All hardware constants live here as `pub const` items on a zero-sized
//! `BoardProfile` struct — no runtime instantiation, no generics.
//! Reference: 技术设计 §2 + change 02 Waveshare ESP-IDF example `bsp_*.h`.

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
    // NB: change 01 set PWR_BTN_PIN=7 thinking it was the EN line. GPIO7 is
    // actually the shared I2C SCL pin on this board (see Waveshare bsp_i2c.h:
    // EXAMPLE_PIN_I2C_SCL = GPIO7). The Waveshare ESP-IDF factory example
    // registers only BOOT(GPIO9) + PLUS(GPIO18) — there is no separate PWR
    // GPIO button (EN is a hardware reset line, not readable as input). PWR
    // long-press handling will be implemented in change 04 InputService via
    // BOOT long-press detection. Value kept as-is to honor change 02 D3 note
    // ("仅追加常量不修改既有常量"); `buttons.rs` skips PWR GPIO init.
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

    // ---- change 02: peripheral pin assignments ----
    // Source: Waveshare ESP32-C6-Touch-LCD-1.54 ESP-IDF example components/esp_bsp/bsp_*.h
    //
    // LCD SPI (SPI2_HOST, 40MHz — ST7789 datasheet max 62MHz, 40MHz for margin)
    pub const LCD_SPI_SCLK_PIN: u8 = 1;
    pub const LCD_SPI_MOSI_PIN: u8 = 2;
    pub const LCD_DC_PIN: u8 = 3;
    pub const LCD_RST_PIN: u8 = 4;
    pub const LCD_CS_PIN: u8 = 11;
    pub const LCD_SPI_FREQ_HZ: u32 = 40_000_000;

    // CST816 touch (I2C addr 0x15, shared with codec I2C bus)
    pub const TOUCH_SDA_PIN: u8 = 8;
    pub const TOUCH_SCL_PIN: u8 = 7;
    pub const TOUCH_IRQ_PIN: u8 = 5;
    pub const TOUCH_I2C_ADDR: u8 = 0x15;

    // I2S0 (full-duplex on a single I2S controller; ES7210 RX uses TDM, ES8311 TX uses STD)
    pub const I2S_MCLK_PIN: u8 = 19;
    pub const I2S_BCLK_PIN: u8 = 20;
    pub const I2S_WS_PIN: u8 = 22;
    pub const I2S_DOUT_PIN: u8 = 23; // → ES8311 DAC
    pub const I2S_DIN_PIN: u8 = 21;  // ← ES7210 ADC

    // Codec I2C (shared with touch bus)
    pub const I2C_SDA_PIN: u8 = 8;
    pub const I2C_SCL_PIN: u8 = 7;
    pub const ES8311_I2C_ADDR: u8 = 0x18;
    pub const ES7210_I2C_ADDR: u8 = 0x40;

    // LEDC PWM for backlight (5kHz, 8-bit duty — 256 levels)
    pub const BACKLIGHT_PWM_FREQ_HZ: u32 = 5_000;
    pub const BACKLIGHT_PWM_DUTY_RES: u8 = 8; // 2^8 = 256

    // ---- Safety / diagnostics (change 16) ----
    pub const FIRMWARE_VERSION: &'static str = "1.0.0";
    pub const SCHEMA_VER: u16 = 1;
    pub const ABNORMAL_BOOT_THRESHOLD: u32 = 3;
    pub const PAIR_JOIN_REASON_INCOMPATIBLE: u8 = 2;

    // ---- Opus PLC / bitrate (change 17 hardware-tuned, defaults per spec) ----
    pub const PLC_CONSECUTIVE_LOSS_THRESHOLD: u8 = 4; // 4 frames / 80ms 静音封底
    pub const OPUS_BITRATE: u32 = 32_000; // 32 kbps VoIP default
}
