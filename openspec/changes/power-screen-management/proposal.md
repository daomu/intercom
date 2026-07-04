## Why

change 04 已定义 `DisplayService` / `InputService` / `PowerService` trait 与 BSP 驱动骨架，change 12 已实现 NVS 冷启动恢复使对讲后台可用。但当前没有任何代码协调"熄屏策略 + 待机时机 + 电量采样 + PA 启停"——屏幕永远亮着耗电、PA 常开有底噪、ADC 原始值跳变、触摸首触会误触发点击。需要本期把 PRD §9 / §26.6 / 技术 §16 的全部省电与电源规则落地，使设备在熄屏后仍能对讲、在非对讲时能真正低功耗待机、电量显示稳定可信。

## What Changes

- 新增 `ScreenPolicy` 策略模块（位于 `src/services/power/screen_policy.rs`）：管理熄屏计时器（默认 30s，从 `Settings::screen_off_sec` 读取）、亮/熄屏状态机、唤醒源路由
- 熄屏唤醒规则：触摸首触 / PWR 短按 / BOOT 短触 → 仅亮屏不触发业务；亮屏后第二次触摸才进入实际操作
- 熄屏对讲保持：熄屏不改变 `IntercomState`；收到他人语音 → 正常播放不亮屏；熄屏按 PTT → 直接进 Talking 不亮屏
- 新增 `StandbyPolicy` 模块（`src/services/power/standby.rs`）：非对讲待机四条件全满足才调 `PowerService::enter_standby()`——系统空闲 + 无前台持续任务 + 无用户交互 + 不影响关键功能
- PA 控制策略：熄屏待机时 `AudioService::stop_playback()`；进入 Listening 前 `start_playback()` 软启动（AudioService 内部 `pa_enable(true)` → 缓冲 1-2 帧 → 启动 I2S）避免爆音。PaController 不直接调用 `pa_enable()`、不自带 20ms/50ms 计时
- 电量采样策略：ADC（GPIO0 ×3 分压）周期采样 + 滑动平均平滑；避开发射 / 大音量瞬时窗口；4 档滞回（升档需连续 N 次达标、降档需连续 M 次低于阈值）；仅暴露 `battery_step()` 4 档图标，不强推百分比、不显示充电中 / 已充满
- Settings 联动：`screen_off_sec` 可在 Settings 页配置（change 07 已提供 UI 入口），本期消费该字段
- 架构预留：低电保护接口骨架（trait 方法声明但不实现完整逻辑），不做真关机、不做完整低电保护

## Capabilities

### New Capabilities
- `power-screen-management`: 熄屏 / 唤醒 / 待机 / PA 启停 / 电量采样与档位显示的完整策略与协调逻辑

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/services/power/` 子模块（`screen_policy.rs`、`standby.rs`、`pa_control.rs`、`mod.rs`）；修改 `src/services/mod.rs` 注册 `pub mod power;`
- **依赖**：deps = `04, 05, 10, 12`——依赖 change 04 的 `DisplayService` / `InputService` / `PowerService` trait 与 BSP 实现；依赖 change 12 的 NVS 恢复使对讲后台在熄屏后可用；依赖 change 05 的 `AudioService::start_playback()` / `stop_playback()`（含 PA 软启动序列）；依赖 change 10 的 `BootPress { screen_was_off }` 字段用于熄屏 PTT 直通
- **系统**：熄屏事件影响 `IntercomService`（不改变 state 但需保持收发）、`AudioService`（PA 开关）、UI 层（暂停 slint 渲染循环以省 CPU）
- **设置**：消费 `Settings::screen_off_sec`（change 03 已定义 NVS 字段）
- **后续变更**：change 17（integration-polish）需整合本期策略到完整启动流程；change 16（safety-diagnostics）可引用 `abnormal_boot_count`
