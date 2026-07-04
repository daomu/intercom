//! intercom firmware entry point (change 01/17 — project skeleton).
//!
//! Initializes ESP-IDF logging, prints a boot banner referencing BoardProfile
//! constants, then blocks forever. The slint `BootWindow` placeholder is
//! validated at compile time by `slint_build::compile("ui/boot.slint")` in
//! `build.rs`, but the slint *runtime* backend is not wired up in change 01:
//! slint's transitive deps (fontique/memmap2) do not yet compile for
//! `target_os = "espidf"`, and a custom `slint::platform::Platform` backend
//! driving the ST7789 LCD is BSP work deferred to change 02
//! (hal-bsp-drivers) per proposal.md Non-Goals.
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

    // Without a slint runtime backend (change 02), block in a low-power loop
    // so the firmware stays alive on the target. The boot banner above is the
    // observable evidence that the skeleton flashed and booted correctly.
    loop {
        FreeRtos::delay_ms(1_000);
    }
}
