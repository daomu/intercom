//! intercom firmware entry point (change 01/17 — project skeleton +
//! change 02/17 — HAL/BSP driver aggregator wired in).
//!
//! Initializes ESP-IDF logging, prints a boot banner, calls `hal::init()` to
//! bring up every peripheral driver in design D10 order (each prints its own
//! init-OK log line), then blocks forever. The slint `BootWindow` placeholder
//! is still validated at compile time by `slint_build::compile`; the slint
//! *runtime* backend + ST7789 frame pump are deferred to change 04
//! (DisplayService) per change 02 proposal.md Non-Goals.
//!
//! Three-task concurrency model (audio / network / UI) is documented in
//! change 01 design.md D10 but NOT created here — later changes instantiate
//! them on top of this skeleton.

#![allow(dead_code)]

mod board_profile;
mod services;
mod intercom;
mod apps;
mod hal;

use board_profile::BoardProfile;
use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::log::EspLogger;

fn main() -> anyhow::Result<()> {
    // Link esp-idf-sys patches (required once at startup).
    esp_idf_svc::sys::link_patches();

    // Bind the `log` crate to ESP-IDF's UART logger.
    EspLogger::initialize_default();

    log::info!(
        "Intercom boot OK, LCD {}×{}, build={}",
        BoardProfile::LCD_W,
        BoardProfile::LCD_H,
        env!("BUILD_TIME"),
    );

    // Bring up every BSP driver. In change 02 these are type-correct stubs
    // (peripheral handle binding lands in changes 04–06 with the consuming
    // Service layers). Real on-device init-order verification is change 02
    // tasks 13.1–13.5 (hardware).
    let _hal = match hal::init() {
        Ok(h) => {
            log::info!("Hal init OK");
            h
        }
        Err(e) => {
            log::error!("{e}");
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Without a slint runtime backend (change 04), block in a low-power loop
    // so the firmware stays alive on the target. Each `Hal::init` step above
    // logged its own OK line — that is the observable evidence the skeleton
    // + BSP aggregator flashed and booted correctly.
    loop {
        FreeRtos::delay_ms(1_000);
    }
}
