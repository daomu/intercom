## Context

`SettingsApp`（`src/apps/settings.rs`）作为纯逻辑状态机已建好且单元测试通过，model 行为正确：`set_brightness` / `set_volume` / `toggle_mute` / `set_screen_off_sec` / `factory_reset_confirm` / `random_name` 都按 spec 实现。change `2026-07-12-ui-render-layer` Phase 2 已补齐 view 层（`settings_view.rs` 7 页 draw + hit_test）。但 controller（main.rs）侧的副作用 wiring 缺失——这是 change 07 与 change 12 之间遗留的接驳缺口，本期补齐。

## Goals / Non-Goals

**Goals:**
- 亮度滑块改变 SHALL 立即生效（调 `HalDisplayService::set_brightness`，而非只写 NVS）
- 恢复出厂二次确认 SHALL 真正清空 NVS 并 `esp_restart()`
- 关于页 4 项诊断数据 SHALL 从 `StorageService` / `SafetyService` 真实读取，SHALL NOT 用硬编码占位
- 单元测试保持通过（model 层签名扩展但不破坏旧测试）

**Non-Goals:**
- 重写 SettingsApp 状态机（行为正确）
- 重写 settings_view.rs（已实现）
- 真音频音量/静音生效（`2026-07-13-wire-audio-pipeline` 范围，本期仅持久化 + 状态栏图标）
- 诊断页扩展 / OTA（change 16 范围）

## Design

### SettingsOutcome 扩展

`SettingsApp::dispatch()` 当前返回 `SettingsOutcome` 枚举。新增 `BrightnessChanged(u8)` 变体：

```
pub enum SettingsOutcome {
    Saved,
    BrightnessChanged(u8),      // 新增
    FactoryResetRequested,       // 已存在
    RandomNameGenerated(String),
    Nop,
}
```

`set_brightness()` 在写 `settings.brightness` + 调 `save_cb` 后返回 `BrightnessChanged(value)`。

### main.rs 接驳

在 `dispatch_settings_action()`（或等价函数）中：

```
match settings_app.dispatch(&ev, &ctx) {
    SettingsOutcome::BrightnessChanged(v) => {
        display.set_brightness(v).ok();
    }
    SettingsOutcome::FactoryResetRequested => {
        log::warn!("Factory reset confirmed — clearing NVS and restarting");
        storage.factory_reset().ok();
        esp_idf_svc::hal::reset::restart();
    }
    SettingsOutcome::Saved | SettingsOutcome::Nop | _ => {}
}
```

### AboutData 注入

main.rs 每 tick（500ms）从 service 快照 `AboutData`：

```
let about = AboutData {
    fw_version: env!("CARGO_PKG_VERSION"),
    last_reset_reason: safety.last_reset_reason(),
    abnormal_restart_count: safety.abnormal_restart_count(),
    secure_boot_enforced: safety.secure_boot_enforced(),
};
settings_app.set_about_data(about);
```

`SettingsApp` 已有 `about: AboutData` 字段，新增 setter。

### BacklightDriver 检查

`BacklightDriver` 在 change 03 已实现 `set_brightness(u8)` 接 LEEDC duty。若接口签名不符（如返回 `()`），本期补齐为 `Result<(), HalError>`。若已符合则仅 wiring。

## Risks

- `esp_restart()` 期间 USB CDC 会断开，monitor 会显示设备重启——预期行为，文档说明
- `factory_reset()` 若 NVS API 失败，仍应 restart（避免半状态），但 log::error 记录失败原因
- `set_brightness(0)` 不应完全熄屏（用户看不到反馈），最小占空比 SHALL ≥ 5%

## Dependencies

- 前置：`2026-07-12-ui-render-layer`（Phase 2 Settings view 已实现）
- 无依赖：与 #2-#7 并行可行
