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
use apps::{new_ui_event_queue, drain_ui_events, push_ui_event, UiEvent, RenderCtx, App};
use apps::shell::{Launcher, AppId};
use apps::settings::{SettingsApp, SettingsOutcome};
use apps::intercom_app::{IntercomApp, PairingAction};
use intercom::safety::{safety_action, BootPlan};
use intercom::heartbeat::{HeartbeatSink, IntercomService, RestoreOutcome};
use intercom::state::{HostPhase, IntercomEvent, IntercomState, JoinPhase};
use intercom::voice::{VoiceAction, VoiceEvent};
use services::audio_service::{AudioService, HalAudioService};
use services::audio_service::thread::{new_voice_rx_queue, AudioThread};
use services::display::{HalDisplayService, DisplayService};
use services::network::{
    new_network_event_queue, EspNowNetworkService, NetworkEvent, NetworkService,
};
use services::power::current_reset_reason;
use services::storage::{NvsStorage, StorageService};
use embedded_graphics::pixelcolor::Rgb565;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;

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
    let mut group = _group;
    let ui_queue = new_ui_event_queue();

    let mut launcher = Launcher::new(
        settings.clone(),
        vec![AppId::Settings, AppId::Intercom],
    );
    let mut settings_app = SettingsApp::new(settings.clone());
    let mut intercom_app = IntercomApp::new(crate::intercom::state::IntercomMode::Clear);

    // ---- Network + intercom runtime (wire-network-runtime) ----------------
    // Instantiate EspNowNetworkService from the ESP-NOW handle, register the
    // recv callback onto a bounded cross-thread queue, then bring up the
    // IntercomService orchestrator and restore any persisted group from NVS.
    let network_q = new_network_event_queue();
    let mut intercom_svc = IntercomService::new();
    let network_svc: Option<Arc<EspNowNetworkService>> = match hal.radio.take_espnow() {
        Some(espnow) => {
            let svc = Arc::new(EspNowNetworkService::new(espnow));
            // Recv callback runs on the ESP-NOW FFI thread — it only pushes to
            // the queue (never touches model state), drop-on-full, non-blocking.
            let q = network_q.clone();
            svc.on_recv(Box::new(move |ev| {
                services::network::push_network_event(&q, NetworkEvent::Recv(ev));
            }));
            if let Err(e) = svc.init(board_profile::BoardProfile::DISCOVERY_CHANNEL) {
                log::error!("network init failed: {e}");
            }
            // Heartbeat sink: unicasts a HEARTBEAT packet to every group peer
            // whenever the tracker decides one is due.
            intercom_svc.set_heartbeat_sink(Box::new(NetHeartbeatSink {
                net: svc.clone(),
                seq: AtomicU16::new(0),
            }));
            log::info!("EspNowNetworkService init OK");
            Some(svc)
        }
        None => {
            log::error!("ESP-NOW handle unavailable (already taken) — network disabled");
            None
        }
    };

    // Restore persisted group (D: no group → ungrouped, never panic).
    let restore_now_ms = unsafe { esp_idf_svc::sys::esp_timer_get_time() } as u64 / 1000;
    match intercom_svc.restore_from_nvs(restore_now_ms, group.as_ref()) {
        RestoreOutcome::NoGroup => {
            log::info!("IntercomService restore: no stored group — starting ungrouped");
        }
        RestoreOutcome::Restore { channel, peers, mode_repr } => {
            if let Some(net) = &network_svc {
                let _ = net.init(channel);
                for (mac, _pub_key) in &peers {
                    // LMK re-derivation is crypto-service scope; add as plain
                    // peers here so unicast fan-out works for this wiring slice.
                    if let Err(e) = net.add_peer(mac, None) {
                        log::error!("restore add_peer {:02x?} failed: {e}", mac);
                    }
                }
            }
            log::info!(
                "IntercomService restore OK: channel={}, {} peers, mode={}",
                channel, peers.len(), mode_repr,
            );
        }
    }

    // ---- Audio pipeline (wire-audio-pipeline) ----
    // Configure the ES7210/ES8311 codecs over the shared I2C bus, then move
    // the I2S driver + audio HAL drivers into HalAudioService (mirrors the
    // display-service ownership handoff above). The audio thread owns an Arc
    // clone and performs all blocking I2S I/O off the main loop.
    if let Err(e) = hal.audio_in.init_codec(&mut hal.i2c) {
        log::error!("ES7210 codec init failed: {e}");
    }
    if let Err(e) = hal.audio_out.init_codec(&mut hal.i2c) {
        log::error!("ES8311 codec init failed: {e}");
    }
    let audio_svc = Arc::new(HalAudioService::new(hal.i2s, hal.audio_in, hal.audio_out));
    // Prime volume/mute from persisted settings (0..100 → 0..255).
    audio_svc.set_volume(vol_to_u8(settings.volume));
    audio_svc.set_mute(settings.muted);

    // Inbound voice queue (network recv → audio thread). Filled by
    // wire-ptt-end-to-end; drained by the audio thread RX path.
    let voice_rx_q = new_voice_rx_queue();

    // Self-test loopback (design §自检 loopback): flip to true to hear your
    // own capture ~80ms delayed. Default off to avoid PA feedback on the
    // bench. Opus follows the cargo feature (PCM passthrough otherwise).
    const AUDIO_LOOPBACK: bool = false;
    let audio_opus = cfg!(feature = "opus");
    match AudioThread::spawn(
        audio_svc.clone(),
        voice_rx_q.clone(),
        // TX sink wired to network send in wire-ptt-end-to-end.
        Box::new(|_seq, _payload| {}),
        audio_opus,
        AUDIO_LOOPBACK,
    ) {
        Ok(_) => log::info!("Audio thread spawned (opus={}, loopback={})", audio_opus, AUDIO_LOOPBACK),
        Err(e) => log::error!("Audio thread spawn failed: {e}"),
    }

    // Input classifiers.
    let mut touch_classifier = services::input::TouchClassifier::new();
    let mut button_classifier = services::input::ButtonClassifier::new();

    // PTT wiring (wire-ptt-end-to-end): TALK_STATE sequence counter, the
    // arbitration/warm-up progression timers, and the ChannelBusy auto-clear
    // deadline. The controller emits ArbitrationTimeout after 50ms and
    // CaptureWarmedUp after a short warm-up guard so clear-mode PTT can
    // progress Idle → Talking → armed without a dedicated timer task.
    let talk_seq = AtomicU16::new(0);
    let mut ptt_timers = PttTimers::default();
    let mut channel_busy_deadline: Option<u64> = None;
    // join_error toast auto-clear deadline (change intercom-ungrouped-ui 7.2).
    let mut join_error_deadline: Option<u64> = None;

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
                // Wake-on-touch: if the screen is off, a real touch wakes the
                // display (backlight + launcher state) and is consumed — it is
                // NOT forwarded to dispatch. This must happen before the
                // TouchClassifier because the classifier suppresses the touch
                // and would never let dispatch_touch (which used to call
                // launcher.wake) run.
                if !screen_on && !matches!(te, hal::touch::TouchEvent::Up { x: 0, y: 0 }) {
                    launcher.wake();
                    display.screen_on();
                    touch_classifier.reset();
                    log::info!("Screen woken on touch (wake-only, not dispatched)");
                } else if let Some(input_ev) = touch_classifier.on_touch(te, screen_on) {
                    if let services::input::InputEvent::Touch(te) = input_ev {
                        // The driver returns Up { 0, 0 } as the idle sentinel
                        // when the finger is held still (no new event). Skip
                        // dispatch entirely for it — otherwise apps that match
                        // the Up arm (e.g. Intercom's PTT up) get spammed every
                        // 50ms while the finger rests.
                        if matches!(te, hal::touch::TouchEvent::Up { x: 0, y: 0 }) {
                            // idle sentinel — drop.
                        } else {
                            log::info!(
                                "Dispatching touch: {:?} (screen_on={})",
                                te, screen_on
                            );
                            dispatch_touch(
                                te,
                                &mut launcher,
                                &mut settings_app,
                                &mut intercom_app,
                                &mut intercom_svc,
                                &display,
                                &storage,
                                &ui_queue,
                                screen_on,
                                now_ms,
                                &mut ptt_timers,
                                &audio_svc,
                                network_svc.as_ref(),
                                &talk_seq,
                            );
                        }
                    }
                }
            }
            Err(e) => {
                // I2C read errors indicate the touch chip is not responding
                // (NOT "no finger present" — that case returns Ok with
                // event=0). Throttle to 1 log per ~5s to avoid flooding.
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
                dispatch_button(ev, &mut launcher, &mut settings_app, &mut intercom_app, now_ms, &mut ptt_timers, &audio_svc, network_svc.as_ref(), &talk_seq, &display);
            }
        }
        if !boot_pressed && prev_boot {
            let events = button_classifier.on_edge(
                hal::buttons::GpioEdgeEvent::BootGpioRelease,
                now_ms,
                !screen_on,
            );
            for ev in events {
                dispatch_button(ev, &mut launcher, &mut settings_app, &mut intercom_app, now_ms, &mut ptt_timers, &audio_svc, network_svc.as_ref(), &talk_seq, &display);
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
            dispatch_button(ev, &mut launcher, &mut settings_app, &mut intercom_app, now_ms, &mut ptt_timers, &audio_svc, network_svc.as_ref(), &talk_seq, &display);
        }
        prev_boot = boot_pressed;
        prev_plus = plus_pressed;

        // ---- 2c. Advance PTT arbitration/warm-up progression ----
        // Emits ArbitrationTimeout (clear-mode) then CaptureWarmedUp so a held
        // PTT can transition Idle → Talking → armed. free-mode presses skip the
        // arbitration step and only schedule warm-up.
        advance_ptt(
            &mut ptt_timers,
            &mut intercom_app,
            now_ms,
            &audio_svc,
            network_svc.as_ref(),
            &talk_seq,
            &display,
        );

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
            // Push volume/mute side-effects to the audio service (ES8311 DAC
            // volume + PA/DAC mute take effect on the next mixed frame).
            audio_svc.set_volume(vol_to_u8(settings.volume));
            audio_svc.set_mute(settings.muted);
        }

        // ---- 3. Drain cross-thread network events → IntercomService ----
        // Recv events update peer liveness/RSSI; a peer coming online enqueues
        // a PeerListChanged so the UI refreshes its roster on the next render.
        let drained: Vec<NetworkEvent> = match network_q.lock() {
            Ok(mut q) => q.drain(..).collect(),
            Err(_) => Vec::new(),
        };
        for nev in drained {
            match nev {
                NetworkEvent::Recv(rev) => {
                    if intercom_svc
                        .on_recv(&rev.src_mac, now_ms, rev.rssi as i32)
                        .is_some()
                    {
                        push_ui_event(&ui_queue, UiEvent::Intercom(IntercomEvent::PeerListChanged));
                    }
                }
                NetworkEvent::SendDone(_ok) => {}
            }
        }

        // Heartbeat tick + offline sweep every 500ms. Newly-offline peers
        // trigger a roster refresh.
        if tick % 10 == 0 {
            let newly_offline = intercom_svc.tick(now_ms);
            if !newly_offline.is_empty() {
                push_ui_event(&ui_queue, UiEvent::Intercom(IntercomEvent::PeerListChanged));
            }
        }

        // Drain UiEvent queue → IntercomApp. Returned VoiceActions are executed
        // against the audio service / network (wire-ptt-end-to-end).
        for ie in drain_ui_events(&ui_queue) {
            // Leaving the group flips the local grouped flag so the render
            // snapshot switches to the ungrouped pages (change
            // intercom-group-info-leave, task 5.1).
            if matches!(ie, IntercomEvent::LeftGroup) {
                group = None;
            }
            let voice_actions = intercom_app.on_intercom_event(ie);
            execute_voice_actions(
                &voice_actions,
                &audio_svc,
                network_svc.as_ref(),
                &talk_seq,
                &display,
            );
        }

        // ChannelBusy auto-recovery: 2s after a busy event, clear the flag and
        // recover the UI to Idle (task 6.1). The visual degrade (orange "BUSY"
        // PTT area) is driven by IntercomUiState::ChannelBusy in the view.
        if intercom_app.channel_busy() {
            match channel_busy_deadline {
                None => channel_busy_deadline = Some(now_ms + 2_000),
                Some(d) if now_ms >= d => {
                    intercom_app.clear_channel_busy();
                    channel_busy_deadline = None;
                    log::info!("ChannelBusy cleared (2s timeout)");
                }
                Some(_) => {}
            }
        } else {
            channel_busy_deadline = None;
        }

        // join_error toast auto-clear: 3s after a JoinRejected set the error,
        // clear it (change intercom-ungrouped-ui 7.2).
        if intercom_app.join_error().is_some() {
            match join_error_deadline {
                None => join_error_deadline = Some(now_ms + 3_000),
                Some(d) if now_ms >= d => {
                    intercom_app.clear_join_error();
                    join_error_deadline = None;
                    log::info!("join_error toast cleared (3s timeout)");
                }
                Some(_) => {}
            }
        } else {
            join_error_deadline = None;
        }

        // VoiceChanger preview sub-state advance (change
        // intercom-voice-changer-preview). One loop iteration ≈ 50ms
        // (FreeRtos::delay_ms(50) at the bottom of the loop).
        {
            let vc_actions = intercom_app.tick_voice_changer(50);
            if !vc_actions.is_empty() {
                execute_voice_actions(
                    &vc_actions,
                    &audio_svc,
                    network_svc.as_ref(),
                    &talk_seq,
                    &display,
                );
                if vc_actions.contains(&VoiceAction::StartPreviewPlayback) {
                    submit_preview_buffer(&audio_svc, intercom_app.preview_buffer());
                }
            }
        }

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
            let audio_stopped = !audio_svc.is_capturing() && !audio_svc.is_playing();
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
                    safe_mode: diag.safe_boot_flag,
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
    intercom_svc: &mut IntercomService,
    display: &HalDisplayService,
    storage: &NvsStorage,
    ui_queue: &apps::UiEventQueue,
    screen_on: bool,
    now_ms: u64,
    ptt_timers: &mut PttTimers,
    audio_svc: &Arc<HalAudioService>,
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
) {
    // If the screen was off, the main loop has already woken it and consumed
    // the wake touch before dispatch_touch is reached, so screen_on is true
    // here. (Kept for defensive clarity — no-op if ever false.)
    if !screen_on {
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
                                // Apply the brightness side-effect immediately
                                // (wire-settings-side-effects): set_brightness
                                // persists to NVS and returns the outcome the
                                // controller uses to drive the backlight.
                                match settings_app.set_brightness(b) {
                                    SettingsOutcome::BrightnessChanged(v) => {
                                        log::info!("Settings: brightness tap → {} (applied)", v);
                                        display.set_brightness(v);
                                    }
                                    SettingsOutcome::Nop => {}
                                }
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
                                if done {
                                    log::warn!(
                                        "Factory reset confirmed — clearing NVS and restarting"
                                    );
                                    if crate::apps::settings::factory_reset(storage).is_err() {
                                        // Best-effort: still restart to avoid
                                        // leaving the device in a half-cleared
                                        // state (spec: NVS 清空失败仍重启).
                                        log::error!(
                                            "factory_reset: NVS clear failed — restarting anyway"
                                        );
                                    }
                                    esp_idf_svc::hal::reset::restart();
                                }
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
                    // Only the grouped Main/VoiceChanger/GroupInfo trio is
                    // swipe-navigable; ungrouped pairing pages ignore swipes.
                    if dx.abs() > 40 && dy.abs() < 20 {
                        let new_page = if dx < 0 {
                            match intercom_app.page() {
                                IntercomPage::Main => IntercomPage::VoiceChanger,
                                IntercomPage::VoiceChanger => IntercomPage::GroupInfo,
                                other => other,
                            }
                        } else {
                            match intercom_app.page() {
                                IntercomPage::GroupInfo => IntercomPage::VoiceChanger,
                                IntercomPage::VoiceChanger => IntercomPage::Main,
                                other => other,
                            }
                        };
                        intercom_app.set_page(new_page);
                        log::info!("Intercom swipe → page {:?}", intercom_app.page());
                    }
                }
                hal::touch::TouchEvent::Down { x, y } => {
                    if let Some(hit) = apps::view::intercom_view::hit_test(x as i32, y as i32, intercom_app.page(), intercom_app.confirming_leave(), intercom_app.vc_state()) {
                        match hit {
                            apps::HitTarget::IntercomPttArea => {
                                log::info!("PTT touch down");
                                if intercom_app.channel_busy() {
                                    log::warn!("PTT pressed during ChannelBusy — allowed, degraded");
                                }
                                let outcome = intercom_app.dispatch(&services::input::InputEvent::Touch(
                                    hal::touch::TouchEvent::Down { x: x, y: y },
                                ));
                                if let Some(pa) = outcome.pairing_action {
                                    route_pairing_action(pa, intercom_svc);
                                }
                                handle_ptt_outcome(
                                    &outcome, now_ms, ptt_timers, audio_svc, network_svc, talk_seq, display,
                                );
                            }
                            // Ungrouped pairing-entry buttons (change intercom-ungrouped-ui).
                            apps::HitTarget::CreateHostButton => {
                                route_pairing_action(intercom_app.tap_create_host(), intercom_svc);
                            }
                            apps::HitTarget::SearchHostsButton => {
                                route_pairing_action(intercom_app.tap_search_hosts(), intercom_svc);
                            }
                            apps::HitTarget::RefreshHostsButton => {
                                route_pairing_action(intercom_app.tap_refresh(), intercom_svc);
                            }
                            apps::HitTarget::HostListItem { idx } => {
                                intercom_app.tap_host_item(idx as usize);
                            }
                            apps::HitTarget::JoinSelectedButton => {
                                if let Some(pa) = intercom_app.tap_join() {
                                    route_pairing_action(pa, intercom_svc);
                                }
                            }
                            apps::HitTarget::PairingBackButton => {
                                route_pairing_action(intercom_app.tap_pairing_back(), intercom_svc);
                            }
                            // GroupInfo leave-group flow (change intercom-group-info-leave).
                            apps::HitTarget::LeaveGroupButton => {
                                intercom_app.tap_leave_group();
                            }
                            apps::HitTarget::CancelLeaveButton => {
                                intercom_app.tap_cancel_leave();
                            }
                            // VoiceChanger preview (change intercom-voice-changer-preview).
                            apps::HitTarget::VoiceEffectButton(effect) => {
                                let actions = intercom_app.tap_voice_effect(effect);
                                execute_voice_actions(
                                    &actions, audio_svc, network_svc, talk_seq, display,
                                );
                            }
                            apps::HitTarget::CancelVoicePreviewButton => {
                                let actions = intercom_app.cancel_voice_preview();
                                execute_voice_actions(
                                    &actions, audio_svc, network_svc, talk_seq, display,
                                );
                            }
                            apps::HitTarget::ConfirmLeaveButton => {
                                if intercom_app.tap_confirm_leave() {
                                    intercom_svc.leave_group();
                                    // Actual leave_group() only stops the tracker;
                                    // synthesize the LeftGroup event so the UI
                                    // switches back to UngroupedHome (task 4.1/5.2).
                                    push_ui_event(
                                        &ui_queue,
                                        apps::UiEvent::Intercom(IntercomEvent::LeftGroup),
                                    );
                                    if let Err(e) = storage.clear_group() {
                                        log::error!("leave_group: clear_group failed: {e:?}");
                                    }
                                    log::info!("Leave group confirmed");
                                }
                            }
                            _ => {}
                        }
                    }
                }
                hal::touch::TouchEvent::Up { x, y } => {
                    log::info!("PTT touch up");
                    let outcome = intercom_app.dispatch(&services::input::InputEvent::Touch(
                        hal::touch::TouchEvent::Up { x: x, y: y },
                    ));
                    if let Some(pa) = outcome.pairing_action {
                        route_pairing_action(pa, intercom_svc);
                    }
                    handle_ptt_outcome(
                        &outcome, now_ms, ptt_timers, audio_svc, network_svc, talk_seq, display,
                    );
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
    intercom_app: &mut IntercomApp,
    now_ms: u64,
    ptt_timers: &mut PttTimers,
    audio_svc: &Arc<HalAudioService>,
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
    display: &HalDisplayService,
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
            // BOOT long press: PTT down. Route through the intercom app so the
            // VoicePttMachine arbitrates + emits VoiceActions. D8: when the
            // screen was off we do NOT wake it — only the audio path arms.
            log::info!("BOOT long press (PTT) screen_was_off={}", screen_was_off);
            if intercom_app.channel_busy() {
                log::warn!("PTT pressed during ChannelBusy — allowed, degraded");
            }
            let outcome = intercom_app
                .dispatch(&services::input::InputEvent::BootPress { screen_was_off });
            handle_ptt_outcome(
                &outcome, now_ms, ptt_timers, audio_svc, network_svc, talk_seq, display,
            );
        }
        services::input::InputEvent::BootRelease => {
            // BOOT release: PTT up.
            log::info!("BOOT release (PTT up)");
            let outcome =
                intercom_app.dispatch(&services::input::InputEvent::BootRelease);
            handle_ptt_outcome(
                &outcome, now_ms, ptt_timers, audio_svc, network_svc, talk_seq, display,
            );
        }
        _ => {}
    }
    launcher.note_activity();
}

/// Phase 4 placeholder: Intercom app view (removed — replaced by draw_intercom).

/// Heartbeat sink backed by `EspNowNetworkService`. Encodes a HEARTBEAT
/// packet and unicasts it to every registered group peer. Invoked by
/// `IntercomService::tick()` when the tracker decides a heartbeat is due.
/// change: wire-network-runtime. The payload identity (sender_id/state/mode)
/// is refined once group identity is plumbed (wire-ptt-end-to-end).
struct NetHeartbeatSink {
    net: Arc<EspNowNetworkService>,
    seq: AtomicU16,
}

impl HeartbeatSink for NetHeartbeatSink {
    fn send_heartbeat(&self) {
        use intercom::packet::{
            encode_heartbeat, HeartbeatPayload, PacketHeader, PacketType, HEADER_LEN, SCHEMA_VER,
        };
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let header = PacketHeader {
            ver: SCHEMA_VER,
            ptype: PacketType::Heartbeat,
            flags: 0,
            seq,
            len: 0,
        };
        let payload = HeartbeatPayload { sender_id: 0, state: 0, mode: 0 };
        let mut buf = [0u8; HEADER_LEN + 3];
        if let Ok(n) = encode_heartbeat(&header, &payload, &mut buf) {
            for mac in self.net.peer_macs() {
                let _ = self.net.send_unicast(&mac, &buf[..n]);
            }
        }
    }
}

/// Route a pairing entry action from the ungrouped UI to the IntercomService
/// state machine (tasks 4.3/4.4). change: wire-network-runtime.
fn route_pairing_action(action: PairingAction, svc: &mut IntercomService) {
    match action {
        PairingAction::StartHost => {
            svc.set_state(IntercomState::Hosting(HostPhase::Discovering));
            log::info!("Pairing: start host");
        }
        PairingAction::SearchHosts => {
            svc.set_state(IntercomState::Joining(JoinPhase::Searching));
            log::info!("Pairing: search hosts");
        }
        PairingAction::Join(mac) => {
            svc.set_state(IntercomState::Joining(JoinPhase::Requesting));
            log::info!("Pairing: join host {:02x?}", mac);
        }
        PairingAction::Cancel => {
            svc.set_state(IntercomState::Idle);
            log::info!("Pairing: cancelled");
        }
    }
}

/// Map a 0..=100 settings volume to the 0..=255 range `AudioService` expects.
fn vol_to_u8(v: u8) -> u8 {
    ((v.min(100) as u16) * 255 / 100) as u8
}

// ---- PTT wiring (wire-ptt-end-to-end) -----------------------------------

/// Arbitration window (clear-mode): time from PttPress to ArbitrationTimeout.
const PTT_ARBITRATION_MS: u64 = 50;
/// Warm-up guard: time from entering Talking to CaptureWarmedUp (ready tone +
/// discarded early frames).
const PTT_WARMUP_MS: u64 = 120;

/// Pending PTT progression deadlines. Driven by the main loop via `advance_ptt`
/// so the clear-mode arbitration + warm-up steps advance without a dedicated
/// timer task.
#[derive(Default)]
struct PttTimers {
    arb_deadline: Option<u64>,
    warm_deadline: Option<u64>,
}

/// Execute the `VoiceAction`s emitted by the PTT state machine against the
/// real services. TALK_STATE packets fan out to every group peer (they fit
/// ESP-NOW's 250B cap); the actual voice audio is captured/encoded/sent by the
/// audio thread (wire-audio-pipeline), so no `SendVoice`-style action exists.
fn execute_voice_actions(
    actions: &[VoiceAction],
    audio_svc: &Arc<HalAudioService>,
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
    display: &HalDisplayService,
) {
    for a in actions {
        match a {
            VoiceAction::None => {}
            VoiceAction::StartCapture => {
                if let Err(e) = audio_svc.start_capture() {
                    log::error!("start_capture failed: {e:?}");
                }
            }
            VoiceAction::StopCapture => {
                if let Err(e) = audio_svc.stop_capture() {
                    log::error!("stop_capture failed: {e:?}");
                }
            }
            VoiceAction::PaEnable { on } => audio_svc.pa_enable(*on),
            VoiceAction::SendTalkState { action } => {
                send_talk_state(network_svc, talk_seq, *action);
            }
            VoiceAction::ScreenOn => display.screen_on(),
            VoiceAction::PlayReadyTone => log::info!("PTT: ready tone (playback deferred)"),
            VoiceAction::PlayBusyTone => log::info!("PTT: busy tone (playback deferred)"),
            VoiceAction::ArmCapture => log::info!("PTT: capture armed"),
            VoiceAction::DisarmCapture => log::info!("PTT: capture disarmed"),
            VoiceAction::StartPreviewPlayback => {
                // VoiceChanger preview playback (change intercom-voice-changer-preview).
                // The processed buffer is submitted separately by the tick site
                // (which owns access to `intercom_app.preview_buffer()`).
                if let Err(e) = audio_svc.start_playback() {
                    log::error!("VC preview start_playback failed: {e:?}");
                }
            }
            VoiceAction::StopPreviewPlayback => {
                if let Err(e) = audio_svc.stop_playback() {
                    log::error!("VC preview stop_playback failed: {e:?}");
                }
            }
        }
    }
}

/// Submit the VoiceChanger preview buffer to the audio service, chunked into
/// 320-sample frames (zero-padding the final partial frame). change:
/// intercom-voice-changer-preview.
///
/// NOTE: real-time playback pacing is handled by the audio thread
/// (wire-audio-pipeline); submitting all frames at once here is a placeholder
/// until on-device playback timing is verified (task 8.4). An empty buffer
/// (capture tap not yet wired) plays silence.
fn submit_preview_buffer(audio_svc: &Arc<HalAudioService>, buf: &[i16]) {
    use crate::services::audio_service::PCM_SAMPLES_PER_FRAME;
    if buf.is_empty() {
        log::info!("VC preview: buffer empty (capture tap deferred) — silent preview");
        return;
    }
    let mut frame = [0i16; PCM_SAMPLES_PER_FRAME];
    for chunk in buf.chunks(PCM_SAMPLES_PER_FRAME) {
        frame.fill(0);
        frame[..chunk.len()].copy_from_slice(chunk);
        if let Err(e) = audio_svc.submit_pcm(0, &frame) {
            log::error!("VC preview submit_pcm failed: {e:?}");
            break;
        }
    }
}

/// Encode a TALK_STATE packet (action 1=start, 0=end) and unicast it to every
/// registered group peer (mirrors `NetHeartbeatSink`). No-op when the radio is
/// unavailable.
fn send_talk_state(
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
    action: u8,
) {
    use intercom::packet::{
        encode_talk_state, PacketHeader, PacketType, TalkStatePayload, HEADER_LEN, SCHEMA_VER,
    };
    let Some(net) = network_svc else {
        return;
    };
    let seq = talk_seq.fetch_add(1, Ordering::Relaxed);
    let header = PacketHeader {
        ver: SCHEMA_VER,
        ptype: PacketType::TalkState,
        flags: 0,
        seq,
        len: 0,
    };
    let payload = TalkStatePayload { sender_id: 0, action };
    let mut buf = [0u8; HEADER_LEN + 2];
    if let Ok(n) = encode_talk_state(&header, &payload, &mut buf) {
        for mac in net.peer_macs() {
            let _ = net.send_unicast(&mac, &buf[..n]);
        }
    }
}

/// After a PTT dispatch, execute its actions and schedule any follow-up
/// progression: clear-mode presses arm the arbitration deadline; free-mode
/// presses go straight to Talking and only need the warm-up guard; releases /
/// arbitration-cancels drop any pending deadlines.
fn handle_ptt_outcome(
    outcome: &apps::intercom_app::IntercomAppOutcome,
    now_ms: u64,
    timers: &mut PttTimers,
    audio_svc: &Arc<HalAudioService>,
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
    display: &HalDisplayService,
) {
    use apps::intercom_app::IntercomUiState;
    execute_voice_actions(&outcome.voice_actions, audio_svc, network_svc, talk_seq, display);

    let is_arbitration = outcome.new_state == IntercomUiState::Idle
        && outcome
            .voice_actions
            .iter()
            .any(|a| matches!(a, VoiceAction::SendTalkState { action: 1 }));
    if is_arbitration {
        timers.arb_deadline = Some(now_ms + PTT_ARBITRATION_MS);
    } else if outcome.new_state == IntercomUiState::PttActive {
        // free-mode press: jumped straight to Talking — schedule warm-up.
        timers.warm_deadline = Some(now_ms + PTT_WARMUP_MS);
    } else if outcome.new_state == IntercomUiState::Idle
        || outcome
            .voice_actions
            .iter()
            .any(|a| matches!(a, VoiceAction::StopCapture))
    {
        // Release or arbitration-cancel: drop pending progression.
        timers.arb_deadline = None;
        timers.warm_deadline = None;
    }
}

/// Advance PTT arbitration/warm-up progression. Called every loop iteration.
fn advance_ptt(
    timers: &mut PttTimers,
    intercom_app: &mut IntercomApp,
    now_ms: u64,
    audio_svc: &Arc<HalAudioService>,
    network_svc: Option<&Arc<EspNowNetworkService>>,
    talk_seq: &AtomicU16,
    display: &HalDisplayService,
) {
    use apps::intercom_app::IntercomUiState;
    if let Some(d) = timers.arb_deadline {
        if now_ms >= d {
            timers.arb_deadline = None;
            let outcome = intercom_app.dispatch_voice(VoiceEvent::ArbitrationTimeout);
            execute_voice_actions(
                &outcome.voice_actions, audio_svc, network_svc, talk_seq, display,
            );
            if outcome.new_state == IntercomUiState::PttActive {
                timers.warm_deadline = Some(now_ms + PTT_WARMUP_MS);
            }
        }
    }
    if let Some(d) = timers.warm_deadline {
        if now_ms >= d {
            timers.warm_deadline = None;
            let outcome = intercom_app.dispatch_voice(VoiceEvent::CaptureWarmedUp);
            execute_voice_actions(
                &outcome.voice_actions, audio_svc, network_svc, talk_seq, display,
            );
        }
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
