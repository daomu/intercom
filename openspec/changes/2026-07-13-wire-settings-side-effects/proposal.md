## Why

`SettingsApp`（change 07）作为纯逻辑状态机已建好，但与运行时 service 的副作用接驳缺失。当前 main loop 只在 SettingsApp `save()` 时持久化到 NVS，三项关键副作用未生效：

1. **亮度调节无效**：`SettingsApp::set_brightness()` 写入 `Settings.brightness` 并调 `save_cb` 持久化，但从未调用 `HalDisplayService::set_brightness()`，屏幕亮度始终停留在 init 值。
2. **恢复出厂无效果**：`SettingsApp::factory_reset_confirm()` 返回 `true`，但 main.rs 未执行 `storage.factory_reset()` + `esp_restart()`，按钮按了等于没按，设备仍带旧配置启动。
3. **关于页数据为空**：`AboutData` 字段全部硬编码占位，固件版本 / 上次复位原因 / 异常重启次数 / 安全启动标记未从 `StorageService` / `SafetyService` 读取，关于页显示假数据。

本期补齐这三条 wiring，使 Settings 页真正生效。不涉及 UI 重写（view 层已由 `2026-07-12-ui-render-layer` Phase 2 实现）。

## What Changes

- **修改 `src/main.rs`**：在 `dispatch_settings_action()` 中处理 `SettingsOutcome::BrightnessChanged(u8)` 调 `display.set_brightness()`；处理 `SettingsOutcome::FactoryResetRequested` 调 `storage.factory_reset()` + `esp_restart()`；每 tick 快照 `AboutData` 经 RenderCtx 注入 SettingsApp
- **修改 `src/apps/settings.rs`**：`SettingsApp` 暴露 `set_brightness` 返回新 `SettingsOutcome::BrightnessChanged(u8)` 变体；`factory_reset_confirm()` 返回 `true` 时 controller 侧执行 reset（model 层不变）；新增 `set_about_data(&mut self, AboutData)` 方法供 controller 注入快照
- **修改 `src/services/storage.rs`**（若缺）：`StorageService::factory_reset()` SHALL 清空 NVS namespace 后返回 `Result<(), StorageError>`；若已存在则仅补 docs
- **修改 `src/apps/view/settings_view.rs`**：关于页 draw 读取 `SettingsApp::about` 字段（已存在但被空数据填充），不重写视图
- **修改 `src/hal/backlight.rs`**（若 `set_brightness` 缺失）：`BacklightDriver::set_brightness(u8)` SHALL 写占空比到 LEDC channel0；若已存在则仅 wiring

## Capabilities

### Modified Capabilities
- `settings-app`: 亮度变更 SHALL 触发 BacklightDriver 生效；恢复出厂 SHALL 触发 NVS 清空 + 重启；关于页 SHALL 显示真实诊断数据
