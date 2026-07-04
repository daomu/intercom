//! Build script for the intercom firmware.
//!
//! Order (per change 01 design.md D2): slint first, then embuild.
//! 1. `slint_build::compile` compiles `ui/boot.slint` into Rust bindings
//!    (module `slint_generated_boot`).
//! 2. `embuild::build::esp_idf` drives the ESP-IDF build system.
//! 3. Emit a `BUILD_TIME` env var (read via `env!("BUILD_TIME")` in main.rs).

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // 1. Compile slint UI → Rust bindings.
    //    The esp-idf target is auto-detected by slint_build; backend-esp-idf feature
    //    selects the embedded renderer at runtime.
    slint_build::compile("ui/boot.slint").expect("slint_build::compile(ui/boot.slint) failed");

    // 2. Drive ESP-IDF (picks up sdkconfig.defaults + partitions.csv).
    embuild::espidf::sysenv::output();

    // 3. Emit build timestamp (seconds since UNIX_EPOCH) for the main crate.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=BUILD_TIME={}", now);

    // Re-run us if the slint file changes.
    println!("cargo:rerun-if-changed=ui/boot.slint");
}
