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

use apps::view::intercom_view::IntercomPage;
use apps::view::status_bar::draw_status_bar;
use apps::view::volume_panel::draw_volume_panel;
use apps::{new_ui_event_queue, drain_ui_events, RenderCtx, App};
use apps::shell::{Launcher, AppId};
use apps::settings::SettingsApp;
use apps::intercom_app::IntercomApp;
use intercom::safety::{safety_action, BootPlan};
use services::display::{HalDisplayService, DisplayService};
use services::power::current_reset_reason;
use services::storage::{NvsStorage, StorageService};
use embedded_graphics::pixelcolor::Rgb565;

fn main() -> anyhow::Result<()> {
    // 1. ESP-IDF logging bootstrap.
    esp_idf_svc::sys::link_patches();
    EspLogger::initialize_default();
    // Give USB CDC time to enumerate so early logs aren't lost.
    FreeRtos::delay_ms(300);

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
    let mut hal = match hal::init() {
        Ok(h) => {
            log::info!("Hal init OK");
            h
        }
        Err(e) => {
            log::error!("{e}");
            return Err(anyhow::anyhow!("{e}"));
        }
    };

    // Wire the display service: move backlight + lcd out of Hal into the
    // service, allocate the framebuffer, and push a boot screen so the
    // panel no longer shows post-init GRAM garbage. The remaining Hal
    // fields (touch/buttons/battery/…) stay in `hal` and are kept alive for
    // later wiring.
    let display = HalDisplayService::new(hal.backlight, hal.lcd);
    display.draw_boot_screen();
    log::info!("Boot screen rendered");

    // 8. Successful full boot — clear diag so next boot starts fresh (D5).
    if let Err(e) = storage.clear_diag() {
        log::error!("clear_diag on boot completion failed: {e:?}");
    }
    log::info!("Boot completed, diag cleared");

    // ---- UI main loop (Phase 3: touch navigation) --------------------------
    // Construct the Launcher + SettingsApp state machines with loaded
    // settings. Touch + button polling drives model dispatch; render loop
    // paints the foreground app view every 500ms.
    let mut settings = _settings;
    let group = _group;
    let ui_queue = new_ui_event_queue();

    let mut launcher = Launcher::new(
        settings.clone(),
        vec![AppId::Settings, AppId::Intercom],
    );
    let mut settings_app = SettingsApp::new(settings.clone());
    let mut intercom_app = IntercomApp::new(crate::intercom::state::IntercomMode::Clear);

    // Input classifiers.
    let mut touch_classifier = services::input::TouchClassifier::new();
    let mut button_classifier = services::input::ButtonClassifier::new();

    // Button edge-detection state (polling-based, not ISR).
    let mut prev_boot = false;
    let mut prev_plus = false;

    // Loop counters: tick at 50ms, render every 10 ticks (500ms),
    // screen-off timer every 20 ticks (1s).
    let mut tick: u32 = 0;
    let mut standby = false;
    let mut touch_err_count: u32 = 0;
    const STANDBY_GRACE_SEC: u32 = 60;

    log::info!("UI main loop starting (50ms poll, 500ms render)");

    loop {
        let now_ms = unsafe { esp_idf_svc::sys::esp_timer_get_time() } as u64 / 1000;
        let screen_on = launcher.screen_on();

        // ---- 1. Poll touch (CST816 via shared I2C) ----
        // Snapshot settings before dispatch to detect changes for NVS persist.
        let settings_before = settings_app.settings().clone();
        let (touch_drv, i2c_drv, buttons_drv) = (&mut hal.touch, &mut hal.i2c, &mut hal.buttons);
        match touch_drv.read_event(i2c_drv) {
            Ok(te) => {
                // Diagnostic: log non-idle touch events only.
                if !matches!(te, hal::touch::TouchEvent::Up { x: 0, y: 0 }) {
                    log::info!("Touch poll Ok: {:?}", te);
                }
                // TouchClassifier suppresses first touch after screen-wake.
                if let Some(input_ev) = touch_classifier.on_touch(te, screen_on) {
                    if let services::input::InputEvent::Touch(te) = input_ev {
                        // Log only non-idle dispatches to avoid flooding.
                        if !matches!(te, hal::touch::TouchEvent::Up { x: 0, y: 0 }) {
                            log::info!("Dispatching touch: {:?} (screen_on={})", te, screen_on);
                        }
                        dispatch_touch(
                            te,
                            &mut launcher,
                            &mut settings_app,
                            &mut intercom_app,
                            screen_on,
                        );
                    }
                }
            }
            Err(e) => {
                // I2C read errors are common when no finger present; log at
                // warn but throttle to 1 per ~5s (100 ticks) to avoid flooding.
                touch_err_count = touch_err_count.saturating_add(1);
                if touch_err_count % 100 == 1 {
                    log::warn!("Touch poll error ({} occurrences): {e}", touch_err_count);
                }
            }
        }

        // ---- 2. Poll buttons (BOOT + PLUS via GPIO) ----
        let boot_pressed = buttons_drv.boot_pressed();
        let plus_pressed = buttons_drv.plus_pressed();

        // Detect edges and feed ButtonClassifier.
        if boot_pressed && !prev_boot {
            let events = button_classifier.on_edge(
                hal::buttons::GpioEdgeEvent::BootGpioPress,
                now_ms,
                !screen_on,
            );
            for ev in events {
                dispatch_button(ev, &mut launcher, &mut settings_app);
            }
        }
        if !boot_pressed && prev_boot {
            let events = button_classifier.on_edge(
                hal::buttons::GpioEdgeEvent::BootGpioRelease,
                now_ms,
                !screen_on,
            );
            for ev in events {
                dispatch_button(ev, &mut launcher, &mut settings_app);
            }
        }
        if plus_pressed && !prev_plus {
            let _ = button_classifier.on_edge(
                hal::buttons::GpioEdgeEvent::PlusGpioPress,
                now_ms,
                !screen_on,
            );
        }
        if !plus_pressed && prev_plus {
            let _ = button_classifier.on_edge(
                hal::buttons::GpioEdgeEvent::PlusGpioRelease,
                now_ms,
                !screen_on,
            );
        }
        // Poll for mid-hold long-press detection.
        let long_events = button_classifier.poll_long_press(now_ms, !screen_on);
        for ev in long_events {
            dispatch_button(ev, &mut launcher, &mut settings_app);
        }
        prev_boot = boot_pressed;
        prev_plus = plus_pressed;

        // ---- 2b. Sync settings: settings_app is the single source of
        // truth (both Settings app edits and button/overlay mute toggles
        // go through settings_app.settings_mut). Propagate to the main
        // `settings` local (used for RenderCtx) and launcher.settings.
        // Persist to NVS when settings changed during this iteration. ----
        settings = settings_app.settings().clone();
        *launcher.settings_mut() = settings.clone();
        if settings != settings_before {
            if let Err(e) = storage.save_settings(&settings) {
                log::error!("save_settings persist failed: {e:?}");
            }
        }

        // ---- 3. Drain UiEvent queue ----
        let _ = drain_ui_events(&ui_queue);

        // ---- 4. Screen-off timer + standby check (every 20 ticks = 1s) ----
        if tick % 20 == 0 {
            match launcher.tick() {
                apps::shell::LauncherTickAction::ScreenOff => {
                    display.screen_off();
                    log::info!("Screen off (inactivity timeout)");
                }
                apps::shell::LauncherTickAction::None => {}
            }
            // Standby policy (spec: 非对讲低功耗待机).
            // Four conditions: (1) intercom idle/ungrouped, (2) no overlay
            // or foreground continuous task, (3) idle ≥ 60s, (4) audio
            // stopped (placeholder — assume true until change 05 wires
            // AudioService).
            let grouped = group.is_some();
            let intercom_idle = !grouped
                || intercom_app.ui_state() == apps::intercom_app::IntercomUiState::Idle;
            let no_fg_task = launcher.overlay().is_none() && intercom_idle;
            let idle = launcher.idle_secs() >= STANDBY_GRACE_SEC;
            let audio_stopped = true; // placeholder (change 05)
            let should_standby = intercom_idle && no_fg_task && idle && audio_stopped;
            if should_standby && !standby {
                standby = true;
                log::info!("Standby: enter (idle {}s)", launcher.idle_secs());
                // TODO: PowerService::enter_standby() once storage ownership
                // is resolved (standby flag tracked locally for now).
            } else if !should_standby && standby {
                standby = false;
                log::info!("Standby: wakeup");
                // TODO: PowerService::wakeup()
            }
        }

        // ---- 5. Render every 10 ticks (500ms) ----
        if tick % 10 == 0 {
            let screen_on = launcher.screen_on();
            if screen_on {
                // Snapshot service state into RenderCtx.
                let ctx = RenderCtx {
                    battery_step: 3,
                    signal_bars: 0,
                    time_hms: (0, 0, 0),
                    is_grouped: group.is_some(),
                    muted: settings.muted,
                    fw_version: BoardProfile::FIRMWARE_VERSION,
                    settings: &settings,
                    safe_mode: false,
                    mode: None,
                    build_time: env!("BUILD_TIME").parse().unwrap_or(0),
                    reset_reason: reset,
                    abnormal_boot_count: diag.abnormal_boot_cnt,
                };

                let _ = display.with_fb(|fb| {
                    fb.fill(Rgb565::new(0x04, 0x06, 0x0C));
                    draw_status_bar(fb, &ctx);
                    match launcher.foreground() {
                        AppId::Launcher => {
                            // Dispatch via App::render (not a direct draw_launcher
                            // call) per ui-render-layer spec requirement C.
                            launcher.render(fb, &ctx);
                        }
                        AppId::Settings => {
                            // Dispatch via App::render (delegates to
                            // settings_view::draw_settings by page).
                            settings_app.render(fb, &ctx);
                        }
                        AppId::Intercom => {
                            // Dispatch via App::render (delegates to
                            // intercom_view::draw_intercom by page).
                            intercom_app.render(fb, &ctx);
                        }
                        AppId::About => {
                            // About is shown via Settings page 5, not a
                            // separate app; fall back to Launcher render.
                            launcher.render(fb, &ctx);
                        }
                    }
                    // Overlay: volume panel (if open).
                    if launcher.overlay() == Some(apps::shell::Overlay::VolumePanel) {
                        draw_volume_panel(fb, &ctx);
                    }
                });
            }
        }

        tick = tick.wrapping_add(1);
        FreeRtos::delay_ms(50);
    }
}

/// Dispatch a touch event to the appropriate model action based on the
/// current foreground app and hit-test result.
fn dispatch_touch(
    te: hal::touch::TouchEvent,
    launcher: &mut Launcher,
    settings_app: &mut SettingsApp,
    intercom_app: &mut IntercomApp,
    screen_on: bool,
) {
    // If screen is off, touch wakes the screen (TouchClassifier already
    // suppressed the first touch). This path handles subsequent touches.
    if !screen_on {
        launcher.wake();
        return;
    }

    // Overlay takes priority (volume panel).
    if launcher.overlay().is_some() {
        match te {
            hal::touch::TouchEvent::Down { x, y } => {
                match apps::view::volume_panel::hit_test(x as i32, y as i32) {
                    Some(apps::HitTarget::VolumeMuteBtn) => {
                        let m = !settings_app.settings().muted;
                        settings_app.set_muted(m);
                        log::info!("Volume panel: mute={}", m);
                    }
                    Some(apps::HitTarget::VolumePanelClose) | None => {
                        launcher.back(); // closes overlay
                        log::info!("Volume panel: closed");
                    }
                    _ => {}
                }
            }
            hal::touch::TouchEvent::Swipe { dx: _, dy: _ } => {}
            _ => {}
        }
        return;
    }

    match launcher.foreground() {
        AppId::Launcher => {
            match te {
                hal::touch::TouchEvent::Down { x, y } => {
                    if let Some(hit) = apps::view::launcher_view::hit_test(x as i32, y as i32) {
                        match hit {
                            apps::HitTarget::LauncherSettingsTile => {
                                log::info!("Touch: Launcher → Settings tile");
                                launcher.launch(AppId::Settings);
                            }
                            apps::HitTarget::LauncherIntercomTile => {
                                log::info!("Touch: Launcher → Intercom tile");
                                launcher.launch(AppId::Intercom);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        AppId::Settings => {
            match te {
                hal::touch::TouchEvent::Swipe { dx, dy } => {
                    // Horizontal swipe: |dx| > 40, |dy| < 20.
                    // Spec: left swipe (dx < 0) → next page, right swipe → prev.
                    if dx.abs() > 40 && dy.abs() < 20 {
                        if dx < 0 {
                            // Swipe left → next page.
                            settings_app.swipe_next();
                            log::info!("Swipe left → Settings page {:?}", settings_app.page());
                        } else {
                            // Swipe right → prev page, or back to Launcher
                            // if on first page.
                            if settings_app.page() as usize == 0 {
                                launcher.back();
                                log::info!("Swipe right → Launcher");
                            } else {
                                settings_app.swipe_prev();
                                log::info!("Swipe right → Settings page {:?}", settings_app.page());
                            }
                        }
                    }
                }
                hal::touch::TouchEvent::Down { x, y } => {
                    use crate::apps::view::settings_view::field;
                    if let Some(crate::apps::HitTarget::SettingsControl { field }) =
                        crate::apps::view::settings_view::hit_test(
                            x as i32,
                            y as i32,
                            settings_app.page(),
                            settings_app.factory_reset_state(),
                        )
                    {
                        match field {
                            field::DEVICE_NAME => {
                                // Text edit entry deferred (no input method yet).
                                log::info!("Settings: device name edit tapped (no-op)");
                            }
                            field::DEVICE_NAME_RANDOM => {
                                let rng = unsafe { esp_idf_svc::sys::esp_random() };
                                let name = SettingsApp::random_device_name(rng);
                                log::info!("Settings: random name → {}", name);
                                settings_app.set_device_name(&name);
                            }
                            field::VOLUME => {
                                // Slider tap: map x [20..220] → 0..100.
                                let v = (((x as i32 - 20).max(0).min(200)) * 100 / 200) as u8;
                                log::info!("Settings: volume tap → {}", v);
                                settings_app.set_volume(v);
                            }
                            field::MUTE => {
                                let m = !settings_app.settings().muted;
                                log::info!("Settings: mute → {}", m);
                                settings_app.set_muted(m);
                            }
                            field::BRIGHTNESS => {
                                let b = (((x as i32 - 20).max(0).min(200)) * 100 / 200) as u8;
                                log::info!("Settings: brightness tap → {}", b);
                                settings_app.set_brightness(b);
                            }
                            field::SCREEN_OFF_5 => {
                                settings_app.set_screen_off_sec(5);
                                log::info!("Settings: screen off 5s");
                            }
                            field::SCREEN_OFF_15 => {
                                settings_app.set_screen_off_sec(15);
                            }
                            field::SCREEN_OFF_30 => {
                                settings_app.set_screen_off_sec(30);
                            }
                            field::SCREEN_OFF_60 => {
                                settings_app.set_screen_off_sec(60);
                            }
                            field::SCREEN_OFF_ALWAYS => {
                                settings_app.set_screen_off_sec(u32::MAX);
                                log::info!("Settings: screen always on");
                            }
                            field::FACTORY_RESET_ARM => {
                                settings_app.factory_reset_arm();
                                log::info!("Settings: factory reset armed");
                            }
                            field::FACTORY_RESET_CANCEL => {
                                settings_app.factory_reset_cancel();
                                log::info!("Settings: factory reset cancelled");
                            }
                            field::FACTORY_RESET_CONFIRM => {
                                let done = settings_app.factory_reset_confirm();
                                log::info!("Settings: factory reset confirm → {}", done);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        AppId::Intercom => {
            match te {
                hal::touch::TouchEvent::Swipe { dx, dy } => {
                    // Spec: left swipe (dx < 0) → next page, right swipe → prev.
                    if dx.abs() > 40 && dy.abs() < 20 {
                        let new_page = if dx < 0 {
                            match intercom_app.page() {
                                IntercomPage::Main => IntercomPage::VoiceChanger,
                                IntercomPage::VoiceChanger => IntercomPage::GroupInfo,
                                IntercomPage::GroupInfo => IntercomPage::GroupInfo,
                            }
                        } else {
                            match intercom_app.page() {
                                IntercomPage::GroupInfo => IntercomPage::VoiceChanger,
                                IntercomPage::VoiceChanger => IntercomPage::Main,
                                IntercomPage::Main => IntercomPage::Main,
                            }
                        };
                        intercom_app.set_page(new_page);
                        log::info!("Intercom swipe → page {:?}", intercom_app.page());
                    }
                }
                hal::touch::TouchEvent::Down { x, y } => {
                    if let Some(hit) = apps::view::intercom_view::hit_test(x as i32, y as i32, intercom_app.page()) {
                        match hit {
                            apps::HitTarget::IntercomPttArea => {
                                log::info!("PTT touch down");
                                let _ = intercom_app.dispatch(&services::input::InputEvent::Touch(
                                    hal::touch::TouchEvent::Down { x: x, y: y },
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                hal::touch::TouchEvent::Up { x, y } => {
                    log::info!("PTT touch up");
                    let _ = intercom_app.dispatch(&services::input::InputEvent::Touch(
                        hal::touch::TouchEvent::Up { x: x, y: y },
                    ));
                }
            }
        }
        _ => {}
    }
    launcher.note_activity();
}

/// Dispatch a button InputEvent to model actions.
fn dispatch_button(
    ev: services::input::InputEvent,
    launcher: &mut Launcher,
    settings_app: &mut SettingsApp,
) {
    match ev {
        services::input::InputEvent::BootShortTap => {
            // BOOT short tap: back button.
            launcher.back();
            log::info!("BOOT short tap → back (now {:?})", launcher.foreground());
        }
        services::input::InputEvent::PlusShortPress => {
            // PLUS short press: toggle volume panel overlay.
            let input_ev = services::input::InputEvent::PlusShortPress;
            let dispatch = launcher.dispatch_input(&input_ev);
            match dispatch {
                apps::shell::InputDispatch::OpenedOverlay(_) => {
                    log::info!("Volume panel opened");
                }
                apps::shell::InputDispatch::ClosedOverlay => {
                    log::info!("Volume panel closed");
                }
                _ => {}
            }
        }
        services::input::InputEvent::PlusLongPress => {
            // PLUS long press: toggle mute (settings_app is single source
            // of truth for settings; synced to launcher + main local
            // after dispatch returns).
            let m = !settings_app.settings().muted;
            settings_app.set_muted(m);
            log::info!("PLUS long press → mute={}", m);
        }
        services::input::InputEvent::BootPress { screen_was_off } => {
            // BOOT long press: PTT (Phase 4/5).
            log::info!("BOOT long press (PTT) screen_was_off={}", screen_was_off);
        }
        services::input::InputEvent::BootRelease => {
            log::info!("BOOT release (PTT up)");
        }
        _ => {}
    }
    launcher.note_activity();
}

/// Phase 4 placeholder: Intercom app view (removed — replaced by draw_intercom).

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
