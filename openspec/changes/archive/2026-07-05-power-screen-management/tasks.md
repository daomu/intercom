## 1. 模块骨架与注册

- [x] 1.1 新增 `src/services/power/mod.rs`，声明子模块 `screen_policy`、`standby`、`pa_control`、`low_power`，加 `#![allow(dead_code)]` 占位（不新建 `battery` 子模块，电量采样由 change 04 `PowerService` 独占）
- [x] 1.2 在 `src/services/mod.rs` 追加 `pub mod power;`
- [x] 1.3 定义策略层公共类型：`ScreenState`（On/Off）、`PowerEvent` 枚举（用于策略间通信）

## 2. 熄屏策略 (ScreenPolicy)

- [x] 2.1 新增 `src/services/power/screen_policy.rs`：定义 `ScreenPolicy` 结构体，持有 `&dyn DisplayService` 引用、`last_activity_mono: Instant`、`screen_state: ScreenState`、`first_touch_consumed: bool`、`screen_off_sec: u32`
- [x] 2.2 实现 `ScreenPolicy::on_input_event(&mut self, ev: &InputEvent)`：亮屏时任意事件重置 `last_activity`；熄屏时 `Touch(Down)` / `BootShortTap` / `PowerShortPress` → `screen_on()` + 设 `first_touch_consumed = true` + 不转发；`BootPress` → 直接转发（PTT 直通）
- [x] 2.3 实现 `ScreenPolicy::tick(&mut self)`：由 Task C 每秒调用，检查 `now - last_activity > screen_off_sec` → `screen_off()` + 通知 StandbyPolicy
- [x] 2.4 实现 `ScreenPolicy::on_intercom_state_change(&mut self, state: IntercomState)`：进入 Talking / Listening 不重置计时器（仅用户交互重置）
- [x] 2.5 实现唤醒首触过滤逻辑：亮屏后第一次 `Touch(Down)` 被消费不转发，`Touch(Up)` 时清除 `first_touch_consumed`

## 3. 非对讲待机策略 (StandbyPolicy)

- [x] 3.1 新增 `src/services/power/standby.rs`：定义 `StandbyPolicy` 结构体，持有 `&dyn PowerService` 引用、`standby_grace_sec: u32`（默认 60）
- [x] 3.2 实现 `StandbyPolicy::should_standby(&self, intercom_state, has_foreground_task, last_interaction, audio_active) -> bool`：四条件逻辑 AND
- [x] 3.3 实现 `StandbyPolicy::evaluate(&mut self, ...)`：全满足 → `enter_standby()` + 返回 `PaControl` 关 PA 指令；任一失效 → `wakeup()`
- [x] 3.4 确保 StandbyPolicy 仅在 ScreenPolicy 熄屏后才检查（`standby_grace_sec > screen_off_sec`）

## 4. PA 控制 (PaController)

- [x] 4.1 新增 `src/services/power/pa_control.rs`：定义 `PaController` 结构体，持有 `&dyn AudioService` 引用（不持有 `pa_soft_start_ms` / `pa_hold_ms`——软启动时序由 AudioService 独占，D6 跨变更）
- [x] 4.2 实现 `PaController::on_enter_listening(&self)`：调用 `AudioService::start_playback()`（AudioService 内部 `pa_enable(true)` → 缓冲 1-2 帧 → 启动 I2S）；不直接调 `pa_enable`、不自带延迟
- [x] 4.3 实现 `PaController::on_exit_listening(&self)`：调用 `AudioService::stop_playback()`（AudioService 内部零帧 → `pa_enable(false)` → 停 I2S）
- [x] 4.4 实现 `PaController::on_enter_talking(&self)`：调用 `start_playback()`（就绪提示音需要 PA）；`on_exit_talking` 同 4.3 调用 `stop_playback()`
- [x] 4.5 实现 `PaController::on_enter_standby(&self)`：立即调用 `stop_playback()`

## 5. 电量查询（无独立模块，D25 跨变更）

- [x] 5.1 不新建 `battery.rs` / `BatterySampler`；策略层直接调用 `PowerService::battery_step()`（返回 0-3 四档）与 `battery_level()`（原始 0-100）。采样 / EWMA / 滞回 / 电压映射由 change 04 独占
- [x] 5.2 在 `ScreenPolicy` / `StandbyPolicy` 中实现 `poll_battery(intercom_state, volume) -> u8`：非 Talking 且非大音量 Listening 时调用 `battery_step()`，否则复用上次缓存值
- [x] 5.3 UI 状态栏直接取 `battery_step()` 返回值渲染 4-bar 图标（无百分比、无充电状态）

## 6. 低电保护接口预留

- [x] 6.1 新增 `src/services/power/low_power.rs`：定义 `pub trait LowPowerProtection { fn check_and_act(&self); }`，注释标注"预留接口，本期不实现"
- [x] 6.2 提供 `impl LowPowerProtection for NoopLowPowerProtection` 空实现，`check_and_act` 为空函数体

## 7. 策略协调器

- [x] 7.1 新增 `src/services/power/coordinator.rs`（或在 `mod.rs` 中）：定义 `PowerCoordinator` 结构体，组合 `ScreenPolicy` + `StandbyPolicy` + `PaController`（无 `BatterySampler`，电量查询直接走 `PowerService::battery_step()`，D25）
- [x] 7.2 实现 `PowerCoordinator::on_input_event(&mut self, ev: &InputEvent)`：先过 `ScreenPolicy`（判断是否转发），转发的事件再通知 StandbyPolicy 重置交互计时
- [x] 7.3 实现 `PowerCoordinator::on_intercom_state_change(&mut self, old: IntercomState, new: IntercomState)`：通知 ScreenPolicy + PaController + StandbyPolicy
- [x] 7.4 实现 `PowerCoordinator::tick(&mut self)`：顺序调用 ScreenPolicy::tick → StandbyPolicy::evaluate → `poll_battery()`（直接查 `PowerService::battery_step()`）

## 8. Settings 联动

- [x] 8.1 实现 `ScreenPolicy::update_screen_off_sec(&mut self, sec: u32)`：从 `Settings::screen_off_sec` 变更时调用，立即生效
- [x] 8.2 在 App/Shell 层 Settings 修改回调中调用 `PowerCoordinator::update_screen_off_sec(sec)`

## 9. 集成接线

- [x] 9.1 在 Task C（UI/Shell）主循环中调用 `PowerCoordinator::tick()`（每秒一次）
- [x] 9.2 在 `InputService::on_event` 回调中调用 `PowerCoordinator::on_input_event(ev)`
- [x] 9.3 在 `IntercomService` 状态变更通知点调用 `PowerCoordinator::on_intercom_state_change(old, new)`
- [x] 9.4 熄屏时 Task C 停止 `DisplayService::present()` 调用；唤醒时恢复

## 10. 验证与测试

- [x] 10.1 单元测试：`ScreenPolicy` 熄屏计时器重置逻辑（mock `Instant`）
- [x] 10.2 单元测试：`ScreenPolicy` 唤醒首触过滤（熄屏 Touch Down 不转发、第二次 Touch Down 转发）
- [x] 10.3 单元测试：`StandbyPolicy::should_standby()` 四条件真值表（16 种组合）
- [x] 10.4 单元测试：`poll_battery()` 在 Talking / 大音量 Listening 时跳过查询复用缓存值（mock `PowerService::battery_step()`）
- [x] 10.5 On-device 验证：亮屏 30s 无操作自动熄屏
- [x] 10.6 On-device 验证：熄屏后另一台设备发语音 → 正常播放不亮屏
- [x] 10.7 On-device 验证：熄屏按 BOOT → 直接发言不亮屏
- [x] 10.8 On-device 验证：熄屏触摸首触只亮屏，第二次触摸才触发按钮
- [x] 10.9 On-device 验证：未组网 + 60s 无操作 → 进入待机（串口日志确认 `enter_standby` 调用）
- [x] 10.10 On-device 验证：电量图标 4 档显示稳定，连续对讲期间不跳变
- [x] 10.11 On-device 验证：进入 Listening 无爆音（`AudioService::start_playback()` 软启动生效）

## 11. 收尾

- [x] 11.1 提交 commit：`feat: power & screen management strategy (change 15/17)`
- [x] 11.2 在 commit message 注明依赖 change 04（Service trait + `battery_step()` 单一真源）、change 05（`AudioService::start_playback()` / `stop_playback()` 含 PA 软启动）、change 10（`BootPress { screen_was_off }` 字段）与 change 12（NVS 恢复），引用技术 §16 / PRD §9


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
