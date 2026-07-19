## 1. 亮度副作用

- [x] 1.1 在 `src/apps/settings.rs` 扩展 `SettingsOutcome` 枚举新增 `BrightnessChanged(u8)` 变体；`set_brightness` 返回该变体
- [x] 1.2 在 `src/hal/backlight.rs` 检查 `BacklightDriver::set_brightness(u8) -> Result<(), HalError>` 存在；若返回 `()` 改为 `Result`（已为 `Result`，无需改动）
- [x] 1.3 在 `src/main.rs` `dispatch_touch` 处理 `BrightnessChanged(v)` 调 `display.set_brightness(v)`，log 失败不中断（`DisplayService::set_brightness` 内部吞错）
- [x] 1.4 验证 `set_brightness(0)` 时实际占空比不低于 5%（防全黑）；在 `BacklightDriver` 层将应用占空比 clamp 至 `MIN_VISIBLE_DUTY = 13`（≈5% of 255）

## 2. 恢复出厂

- [x] 2.1 在 `src/services/storage.rs` 确认 `factory_reset()` 清空 NVS namespace 已实现（`apps::settings::factory_reset` 调 `reset_settings`/`clear_group`/`clear_diag`，均已存在）
- [x] 2.2 在 `src/main.rs` 处理二次确认：`factory_reset(storage)` 后 `esp_idf_svc::hal::reset::restart()`
- [x] 2.3 `factory_reset()` 失败时 log::error 记录但仍 `restart()`（避免半状态）
- [x] 2.4 单元测试：`factory_reset_two_step` 已验证 `factory_reset_confirm()` 在二次确认后返回 true；真实 `esp_restart()` 仅硬件验证

## 3. 关于页真实数据

- [x] 3.1 关于页真实数据经 `RenderCtx` 每 tick 快照注入（实际架构用 `RenderCtx` 而非 `SettingsApp::about` 字段，满足 spec "controller 每 tick 从 service 快照注入" 要求）
- [x] 3.2 诊断数据从 `storage.load_diag()`（`DiagInfo`）+ `current_reset_reason()` 读取，字段已全部可用
- [x] 3.3 在 `src/main.rs` 每 tick 渲染时构建 `RenderCtx`：`reset_reason` / `abnormal_boot_count` / `safe_mode`(← 修正原硬编码 `false` 为 `diag.safe_boot_flag`) / `fw_version`
- [x] 3.4 `fw_version` 用 `BoardProfile::FIRMWARE_VERSION`（= `env!("CARGO_PKG_VERSION")`）编译期注入
- [ ] 3.5 验证关于页 draw 显示真实数据（需硬件）

## 4. 构建验证

- [ ] 4.1 `cargo build` 通过（需 ESP 交叉工具链环境，未在本环境验证）
- [ ] 4.2 `cargo test --lib` 通过（model 层单元测试签名未破坏；需可运行 host 测试的环境验证）
- [ ] 4.3 `cargo run` 验证：滑动亮度条屏幕变亮/变暗；二次确认恢复出厂后设备重启；关于页显示版本号（需硬件）
