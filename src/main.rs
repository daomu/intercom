//! intercom firmware entry point.
//!
//! Boot flow (safety-diagnostics change 16):
//! 1. link_patches + EspLogger
//! 2. NvsStorage::new()
//! 3. Read reset_reason via `power::current_reset_reason()` (before Hal init)
//! 4. If reset != PowerOn: storage.inc_abnormal_boot() + set_last_reset_reason
//! 5. Load diag, compute BootPlan via SafetyStateMachine
//! 6. If SafeBoot: set_safe_boot_flag(true), skip Hal/Intercom init, enter
//!    Settings-only mode (TODO: slint Settings App)
//! 7. Else: Hal::init(), load_settings/load_group with schema_ver fallback,
//!    enter main loop
//! 8. On successful full boot: storage.clear_diag()
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

use intercom::safety::{safety_action, BootPlan};
use services::power::current_reset_reason;
use services::storage::{NvsStorage, StorageService};

fn main() -> anyhow::Result<()> {
    // 1. ESP-IDF logging bootstrap.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();

    log::info!(
        "Intercom boot OK, LCD {}×{}, fw={}, build={}",
        BoardProfile::LCD_W,
        BoardProfile::LCD_H,
        BoardProfile::FIRMWARE_VERSION,
        env!("BUILD_TIME"),
    );

    // 2. NVS storage (needed for safety-diagnostics flow before Hal init).
    let storage = match NvsStorage::new() {
        Ok(s) => s,
        Err(e) => {
            log::error!("NvsStorage init failed: {e:?} — continuing with no storage");
            // Without storage we can't run the safety flow; fall back to a
            // bare Hal init + idle loop so the device still boots.
            return bare_boot();
        }
    };

    // 3. Read reset reason early (D2: before BSP init).
    let reset = current_reset_reason();
    log::info!("Reset reason: {} ({})", reset, reset.to_code());

    // 4. Increment abnormal_boot_cnt for non-PowerOn resets (D2).
    if intercom::safety::should_inc_abnormal(reset) {
        if let Err(e) = storage.inc_abnormal_boot() {
            log::error!("inc_abnormal_boot failed: {e:?}");
        }
    }
    // Persist the reset reason code to diag namespace (D2).
    if let Err(e) = storage.set_last_reset_reason(reset.to_code()) {
        log::error!("set_last_reset_reason failed: {e:?}");
    }

    // 5. Load diag + compute boot plan.
    let diag = storage.load_diag();
    log::info!(
        "Diag: abnormal_boot_cnt={}, safe_boot_flag={}, last_reset_reason={}",
        diag.abnormal_boot_cnt,
        diag.safe_boot_flag,
        diag.last_reset_reason,
    );

    let action = safety_action(reset, diag.abnormal_boot_cnt, diag.safe_boot_flag);
    let plan = BootPlan::from_action(action, reset);

    log::info!(
        "BootPlan: action={:?}, should_inc={}, start_intercom={}, settings_only={}",
        plan.action, plan.should_inc, plan.start_intercom, plan.settings_only,
    );

    // 6. Safe-boot branch: skip Hal/Intercom init.
    if plan.settings_only {
        if let Err(e) = storage.set_safe_boot_flag(true) {
            log::error!("set_safe_boot_flag(true) failed: {e:?}");
        }
        log::warn!("Safe-boot mode entered — Settings App only (slint UI deferred)");
        // TODO: launch Settings-only App once slint runtime backend is wired.
        // For now, idle so the device stays alive and reachable via serial.
        loop {
            FreeRtos::delay_ms(1_000);
        }
    }

    // 7. Normal boot: schema_ver fallback for sys/group, then Hal init.
    // load_settings() returns Settings (never errors — falls back to default
    // internally per apply_schema_rule). If the schema was incompatible, the
    // storage layer already logged a warn and returned defaults; we persist
    // the defaults here to overwrite the stale schema.
    let _settings = storage.load_settings();
    if let Err(e) = storage.save_settings(&_settings) {
        log::warn!("save_settings (schema refresh) failed: {e:?}");
    }

    let _group = match storage.load_group() {
        Some(g) => Some(g),
        None => {
            log::warn!("load_group returned None (no group or schema mismatch) — clearing");
            if let Err(e) = storage.clear_group() {
                log::error!("clear_group fallback failed: {e:?}");
            }
            None
        }
    };

    // Bring up every BSP driver (real peripheral bindings as of change 02+).
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

    // 8. Successful full boot — clear diag so next boot starts fresh (D5).
    if let Err(e) = storage.clear_diag() {
        log::error!("clear_diag on boot completion failed: {e:?}");
    }
    log::info!("Boot completed, diag cleared");

    // Without a slint runtime backend (change 04), block in a low-power loop
    // so the firmware stays alive on the target. Real app dispatch + 3-task
    // model lands when slint + Services are wired together.
    loop {
        FreeRtos::delay_ms(1_000);
    }
}

/// Bare-boot fallback when NvsStorage is unavailable: skip the safety flow
/// entirely, just init Hal (best-effort) and idle.
fn bare_boot() -> anyhow::Result<()> {
    if let Err(e) = hal::init() {
        log::error!("Hal init (bare) failed: {e}");
        return Err(anyhow::anyhow!("{e}"));
    }
    loop {
        FreeRtos::delay_ms(1_000);
    }
}
