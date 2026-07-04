## Context

change 04 已定义三个电源相关 Service trait（技术 §3.6）：
- `DisplayService`：`screen_on()` / `screen_off()` / `is_screen_on()` / `set_brightness()`
- `InputService`：回调式 `InputEvent` 枚举，含 `BootShortTap`（唤醒）、`PowerShortPress`（亮/熄屏）、`Touch(TouchEvent)` 等
- `PowerService`：`battery_level()` / `battery_step()` / `enter_standby()` / `wakeup()`

change 12 已实现 NVS 冷启动恢复——重启后 `IntercomService::restore_from_nvs(g)` 使设备直接进入 `Grouped(Idle)` 后台待机，不依赖空中交互。这意味着熄屏后对讲链路（ESP-NOW 接收 + 解码 + 播放）必须持续运行。

当前缺失的是将这些 trait **协调**起来的策略层：什么时候熄屏、谁唤醒、唤醒后第一次触摸怎么过滤、PA 什么时候开关、ADC 怎么平滑、什么条件下才真正进低功耗待机。这些规则分散在 PRD §9 / §26.6 与技术 §16，需要本期统一实现。

## Goals / Non-Goals

**Goals:**
- 熄屏计时器：默认 30s 无操作自动熄屏，时间从 `Settings::screen_off_sec` 读取，Settings 可配置
- 唤醒源：触摸首触 / PWR 短按 / BOOT 短触——仅亮屏，不触发业务点击
- 熄屏对讲保持：熄屏后 ESP-NOW 接收 + 解码 + 播放不受影响；收到他人语音不亮屏；熄屏按 PTT 直接进 Talking 不亮屏
- 非对讲待机：四条件全满足（系统空闲 + 无前台持续任务 + 无用户交互 + 不影响关键功能）才调 `enter_standby()`
- PA 控制：熄屏待机时 PA 关；进入 Listening 前软启动避免爆音
- 电量采样：ADC 平滑 + 滞回 4 档；避开 TX / 大音量瞬时；仅 4 档图标
- 架构预留：低电保护 trait 接口骨架（不实现完整逻辑）

**Non-Goals:**
- 真关机（硬件不支持，PRD §2.4 明确排除）
- 完整低电保护策略（仅预留接口，不实现自动降频 / 强制熄屏等完整逻辑）
- 充电状态显示（PRD §2.4 明确不作为正式能力）
- 电量百分比数字显示（PRD §9.1 仅 4 档图标）
- OTA 实际下载与电源管理集成（仅分区预留，change 01 已处理）
- slint 渲染循环的具体暂停实现细节（由 change 17 integration-polish 整合）

## Decisions

### D1：策略层独立模块，不侵入 Service trait
`ScreenPolicy` / `StandbyPolicy` / `PaController` 各为独立结构体，持有对 `DisplayService` / `InputService` / `PowerService` / `AudioService` trait 对象的引用。策略逻辑集中在策略层，Service trait 保持薄接口。电量采样不再独立模块——直接复用 `PowerService::battery_step()`（change 04 为唯一真源，D25）。备选：把策略塞进 `PowerService` 实现——导致 trait 膨胀且不可单独测试，排除。

### D2：熄屏计时器 = 单调时钟 + 活动重置
`ScreenPolicy` 内部维护 `last_activity_mono: Instant`，每次收到 `InputEvent`（任何类型）或 `IntercomState` 转入 `Talking` / `Listening` 时重置。Task C（UI/Shell，优先级 5）每秒检查一次：`now - last_activity > screen_off_sec` → 调 `DisplayService::screen_off()` + 通知 `StandbyPolicy`。备选：硬件定时器中断——ESP32-C6 定时器资源有限且与音频 task 抢占，排除。

### D3：唤醒首触过滤 = 状态标志位
`ScreenPolicy` 内部 `screen_state: enum { On, Off }`。熄屏时收到 `Touch(Down)` / `BootShortTap` / `PowerShortPress` → 调 `screen_on()` + 设 `first_touch_consumed = true`，**不转发**该事件给 UI / Intercom 层。亮屏后下一次 `Touch(Down)` 才正常转发。`first_touch_consumed` 在亮屏后第一个 Touch Up 时清除。备选：延迟转发 + 去抖——逻辑复杂且 slint 侧难以配合，排除。

### D4：熄屏 PTT 直通 = 绕过屏幕策略
熄屏时收到 `BootPress`（PTT 按下）→ **不亮屏**，直接转发给 `IntercomService::ptt_press()`。这与 D3 的"首触只亮屏"不矛盾：`BootPress` 是长按事件（≥50ms），`BootShortTap` 是短按事件，两者在 `InputService` BSP 层已区分。熄屏 PTT 走 `BootPress` 路径，不经过唤醒过滤。

### D5：非对讲待机四条件 = 逻辑 AND
`StandbyPolicy::should_standby()` 返回 true 当且仅当：
1. `IntercomState == 未组网` 或 `Grouped(Idle)`（系统空闲）
2. 无前台 App 持续任务（无正在进行的组队、无音量面板弹出、无 Launcher 动画）
3. `now - last_user_interaction > STANDBY_GRACE_SEC`（无用户交互，默认 60s，长于熄屏 30s）
4. `AudioService` 采集/播放均停止（不影响关键功能）

条件 1-4 全满足 → 调 `PowerService::enter_standby()` + `AudioService::stop_playback()`（AudioService 内部关 PA）。任一条件失效 → `PowerService::wakeup()`。备选：仅按超时——可能在组队中误进待机导致对讲中断，排除。

### D6：PA 软启动 = AudioService start/stop_playback 拥有
PaController 调用 `AudioService::start_playback()` / `stop_playback()`，不直接调用 `pa_enable()`、不自带 20ms/50ms 计时。AudioService（change 05，D6 跨变更决策）内部序列：`start_playback` = `pa_enable(true)` → 缓冲 1-2 帧 → 启动 I2S 输出；`stop_playback` = 输出零帧 1-2 帧 → `pa_enable(false)` → 停 I2S。BSP `pa_enable`（change 02）为纯 GPIO 翻转，无时序、无软启动。进入 `Listening` / `Talking` 前调 `start_playback()`；退出后调 `stop_playback()`。备选：PaController 自管 20ms/50ms 计时——时序与音频缓冲耦合差、跨层越权，排除。

### D7：电量采样 = PowerService 单一真源（D25 跨变更）
不新建独立 `BatterySampler` 模块。`ScreenPolicy` / `StandbyPolicy` 直接调 `PowerService::battery_step()`（返回 0-3 四档，change 04 拥有 ADC 平滑 + EWMA + ±0.1V 滞回 + 电压映射）与 `PowerService::battery_level()`（原始 0-100）。策略层仅在需要"避开 TX / 大音量瞬时"时由调用方按 `IntercomState` 判断是否跳过本次查询并复用上次值。备选：策略层独立滑动窗口 + 滞回——与 change 04 双重平滑、档位口径分裂，排除。

### D8：低电保护 = trait 接口预留不实现
定义 `pub trait LowPowerProtection { fn check_and_act(&self); }`，空实现返回。后续版本可填充自动降频 / 强制熄屏 / 低电告警逻辑。本期不实现任何逻辑，仅声明接口与占位实现。备选：不预留——后续变更需改动架构，排除。

### D9：熄屏时 slint 渲染 = 停止 present 但不销毁
熄屏时 `DisplayService::screen_off()` 关背光（GPIO6 LEDC PWM = 0）。Task C 停止调用 `DisplayService::present()`，但 slint Window 对象保留，唤醒后直接恢复渲染。备选：销毁 slint Window 重建——唤醒延迟大且可能丢状态，排除。

## Cross-references

### 熄屏 PTT 互补（与 change 10，D45）
change 10 的 `BootPress { screen_was_off: bool }`（`screen_was_off=true` 分支）在不唤醒屏幕的前提下触发 PTT。change 15 的 `ScreenPolicy` 收到 BSP 的 `BootGpioPress`（raw GPIO 边沿，D5）时将其转发给 `InputService` 做短/长按分类，**不**调用 `screen_on()`；`InputService` 分类为 `BootPress` 后由 `IntercomService::ptt_press()` 处理，仍不亮屏。两者互补不冲突：change 10 负责 PTT 语义与 `screen_was_off` 字段语义，change 15 负责屏幕状态机与唤醒过滤的边界。亮屏后 `BootPress { screen_was_off=false }` 正常进入 Talking。

### BootPress screen_was_off 字段来源（与 change 04，D35）
`InputEvent::BootPress` 携带 `screen_was_off: bool` 字段由 change 04 定义（D35）。`InputService` 在分类时查询 `ScreenPolicy::is_screen_on()`（或等价状态）填入该字段。change 15 不重新定义该字段，仅消费。

## Risks / Trade-offs

- **[熄屏后 ESP-NOW 接收延迟增大]** → Task B（网络业务，优先级 12）不因熄屏降级；若实测延迟超标，在 `enter_standby()` 中保持 ESP-NOW radio 常开不进轻睡眠
- **[ADC 采样避开 TX 窗口可能漏采]** → 策略层仅在 TX / 大音量瞬时跳过查询 `battery_step()`，复用上次值；change 04 内部 2s 间隔 + EWMA 足以覆盖下次空闲采样
- **[AudioService 软启动缓冲 1-2 帧增加 Listening 首帧延迟]** → 抖动缓冲初始水位 3 帧（60ms）已吸收此延迟，用户感知不明显；PaController 不再自管计时
- **[唤醒首触过滤可能导致 slint 收不到 Touch Down]** → 过滤仅针对熄屏→亮屏的第一次 Touch；亮屏后正常转发，slint 侧无感知
- **[StandbyPolicy 与 ScreenPolicy 竞争]** → StandbyPolicy 在 ScreenPolicy 熄屏之后才检查（条件 3 的 grace > screen_off_sec），无竞争
- **[低电保护接口预留但无实现可能误导后续开发]** → trait 注释明确标注"预留接口，本期不实现"

## Migration Plan

无既有运行时策略需要迁移。部署步骤：
1. change 04 已提供 Service trait + BSP 驱动 → 本期在其上构建策略层
2. change 12 已提供 NVS 恢复 → 本期确保熄屏后恢复的对讲链路不被策略层干扰
3. 本期实现完成后，change 17 integration-polish 将策略层接入完整启动流程

回滚：`git revert <commit>`，恢复到无策略层状态（屏幕常亮、PA 常开、ADC 无平滑）。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
