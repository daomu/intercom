## ADDED Requirements

### Requirement: 自动熄屏计时
系统 SHALL 维护一个熄屏计时器：自最后一次用户交互（触摸 / 按键 / PTT）起，经过 `Settings::screen_off_sec` 指定的秒数后自动调用 `DisplayService::screen_off()`。默认熄屏时间 SHALL 为 30 秒。熄屏时间 SHALL 可通过 Settings 页面配置。收到他人语音或 PTT 按下 SHALL NOT 重置熄屏计时器（仅用户主动交互重置）。

#### Scenario: 默认超时熄屏
- **WHEN** 设备亮屏且 30 秒内无任何用户交互（触摸 / 按键 / PTT）
- **THEN** 系统调用 `DisplayService::screen_off()` 关闭背光，屏幕进入熄屏状态

#### Scenario: 用户交互重置计时
- **WHEN** 在熄屏计时未到期前发生任意用户交互（Touch / BootPress / BootShortTap / PowerShortPress / PlusShortPress）
- **THEN** 熄屏计时器重置为 0，重新开始计时

#### Scenario: Settings 修改熄屏时间
- **WHEN** 用户在 Settings 页面将 `screen_off_sec` 从 30 改为 60
- **THEN** 下一次熄屏计时按 60 秒执行，无需重启

#### Scenario: 收到语音不重置计时
- **WHEN** 设备亮屏且收到他人语音帧
- **THEN** 熄屏计时器不被重置，仍按原计时熄屏

### Requirement: 唤醒源与首触过滤
系统 SHALL 支持三种唤醒源：触摸首触、PWR 短按、BOOT 短触。熄屏状态下收到任一唤醒源时 SHALL 仅执行 `DisplayService::screen_on()` 亮屏，SHALL NOT 将该事件转发给 UI 层或 Intercom 层触发业务操作。亮屏后的第一次触摸 SHALL 被消费用于"确认唤醒"，不触发点击；第二次触摸起 SHALL 正常转发。

#### Scenario: 熄屏触摸首触只亮屏
- **WHEN** 屏幕熄屏状态下发生 `Touch(Down)` 事件
- **THEN** 系统调用 `screen_on()` 点亮背光，该 `Touch(Down)` 事件不转发给 UI 层，不触发任何按钮点击

#### Scenario: 亮屏后第二次触摸正常操作
- **WHEN** 唤醒亮屏后用户进行第二次触摸
- **THEN** 该 `Touch(Down)` 事件正常转发给 UI 层，触发对应按钮或交互

#### Scenario: PWR 短按唤醒
- **WHEN** 屏幕熄屏状态下发生 `PowerShortPress` 事件
- **THEN** 系统调用 `screen_on()` 亮屏，不触发其他业务操作

#### Scenario: BOOT 短触唤醒
- **WHEN** 屏幕熄屏状态下发生 `BootShortTap` 事件
- **THEN** 系统调用 `screen_on()` 亮屏，不触发 PTT

### Requirement: 熄屏对讲保持
熄屏 SHALL NOT 改变 `IntercomState`，对讲收发链路 SHALL 继续运行。熄屏时收到他人语音 SHALL 正常播放且 SHALL NOT 亮屏。熄屏时按 PTT（`BootPress`）SHALL 直接进入 Talking 发言且 SHALL NOT 亮屏。

#### Scenario: 熄屏收到语音正常播放
- **WHEN** 屏幕熄屏且 `IntercomState == Grouped(Idle)`，收到他人语音帧
- **THEN** 系统进入 `Grouped(Listening)`，音频正常解码播放，屏幕保持熄屏不亮

#### Scenario: 熄屏按 PTT 直接发言
- **WHEN** 屏幕熄屏且 `IntercomState == Grouped(Idle)`，用户按下 BOOT（`BootPress`）
- **THEN** 系统直接调用 `IntercomService::ptt_press()` 进入 `Grouped(Talking)`，屏幕保持熄屏不亮

#### Scenario: 熄屏对讲链路不中断
- **WHEN** 屏幕从亮屏转为熄屏时 `IntercomState == Grouped(Listening)`
- **THEN** ESP-NOW 接收、Opus 解码、音频播放链路不中断，播放持续正常

### Requirement: 非对讲低功耗待机
系统 SHALL 在以下四个条件同时满足时调用 `PowerService::enter_standby()` 进入低功耗待机：(1) `IntercomState` 为未组网或 `Grouped(Idle)`；(2) 无前台持续任务（无组队进行中、无音量面板弹出）；(3) 无用户交互超过 `STANDBY_GRACE_SEC`（默认 60 秒）；(4) 音频采集与播放均停止。任一条件失效时 SHALL 调用 `PowerService::wakeup()` 退出待机。

#### Scenario: 四条件全满足进入待机
- **WHEN** 设备未组网、无前台任务、60 秒无用户交互、音频停止
- **THEN** 系统调用 `PowerService::enter_standby()` 进入低功耗待机

#### Scenario: 待机中被唤醒
- **WHEN** 设备处于低功耗待机，用户触摸屏幕
- **THEN** 系统调用 `PowerService::wakeup()` 退出待机并亮屏

#### Scenario: 对讲模式不进待机
- **WHEN** `IntercomState == Grouped(Listening)` 且无用户交互超过 60 秒
- **THEN** 系统不进入低功耗待机，保持对讲收发能力

### Requirement: PA 控制与爆音抑制
系统 SHALL 在熄屏待机时调用 `AudioService::stop_playback()` 关闭 PA（AudioService 内部：输出零帧 → `pa_enable(false)` → 停 I2S）。进入 `Listening` / `Talking` 前 SHALL 调用 `AudioService::start_playback()`（AudioService 内部：`pa_enable(true)` → 缓冲 1-2 帧 → 启动 I2S 输出）软启动避免爆音。退出 `Listening` / `Talking` 后 SHALL 调用 `stop_playback()`。PaController SHALL NOT 直接调用 `pa_enable()`，SHALL NOT 自带 20ms / 50ms 计时——软启动序列由 AudioService（change 05）独占。

#### Scenario: 熄屏待机关 PA
- **WHEN** 系统进入低功耗待机
- **THEN** `AudioService::stop_playback()` 被调用，AudioService 内部关闭 PA

#### Scenario: 进入 Listening 前软启动 PA
- **WHEN** 系统从 `Grouped(Idle)` 转入 `Grouped(Listening)`
- **THEN** 调用 `AudioService::start_playback()`，AudioService 内部先 `pa_enable(true)` 并缓冲 1-2 帧再推送解码 PCM，避免爆音

#### Scenario: 退出 Listening 延迟关 PA
- **WHEN** 系统从 `Grouped(Listening)` 转回 `Grouped(Idle)`
- **THEN** 调用 `AudioService::stop_playback()`，AudioService 内部输出零帧 1-2 帧后再 `pa_enable(false)`，避免尾音爆音

### Requirement: 电量档位显示
系统 SHALL 通过 `PowerService::battery_step()` 获取 4 档电量图标（0-3），采样、平滑、EWMA、电压映射与 ±0.1V 滞回均由 change 04 的 `PowerService` 独占实现（单一真源）。策略层（`ScreenPolicy` / `StandbyPolicy`）SHALL 直接调用 `battery_step()` / `battery_level()`，SHALL NOT 重建独立的 `BatterySampler` 模块。策略层 SHALL 在 `IntercomState == Talking` 或音量大于阈值的 `Listening` 时跳过本次查询并复用上次值，避免发射瞬时电流干扰。系统 SHALL NOT 显示百分比数字、SHALL NOT 显示充电中 / 已充满状态。

#### Scenario: 平滑电量显示
- **WHEN** ADC 原始值在短时间内从 40 跳变到 60 再回到 45
- **THEN** `PowerService::battery_step()` 内部 EWMA 平滑后输出稳定不跳变

#### Scenario: 滞回避免档位跳变
- **WHEN** 均值在档位边界附近波动
- **THEN** change 04 的 ±0.1V 滞回保证档位不频繁跳变

#### Scenario: 发射时跳过查询
- **WHEN** `IntercomState == Grouped(Talking)` 且策略层轮询电量
- **THEN** 本次 `battery_step()` 查询被跳过，使用上次缓存值，避免发射瞬时电流干扰

#### Scenario: 仅 4 档图标无百分比
- **WHEN** UI 层请求电量显示
- **THEN** 获取 `battery_step()` 返回 0-3 的 4 档值，无百分比数字、无充电状态

### Requirement: 低电保护接口预留
系统 SHALL 定义 `LowPowerProtection` trait 接口（`fn check_and_act(&self)`），作为低电保护策略的架构预留。本期 SHALL NOT 实现任何完整低电保护逻辑（自动降频 / 强制熄屏 / 低电告警），SHALL 仅提供空实现占位。系统 SHALL NOT 支持软件真关机（硬件不支持）。

#### Scenario: 接口存在但无逻辑
- **WHEN** 调用 `LowPowerProtection::check_and_act()`
- **THEN** 方法存在且可调用，但不执行任何实际低电保护动作

### Requirement: 熄屏时 UI 渲染暂停
熄屏时系统 SHALL 停止调用 `DisplayService::present()` 提交绘制指令，但 SHALL NOT 销毁 slint Window 对象。唤醒后 SHALL 直接恢复 `present()` 调用与渲染循环，无需重建 UI 状态。

#### Scenario: 熄屏停止渲染
- **WHEN** 屏幕从亮屏转为熄屏
- **THEN** Task C 停止调用 `DisplayService::present()`，slint Window 对象保留在内存中

#### Scenario: 唤醒恢复渲染
- **WHEN** 屏幕从熄屏转为亮屏
- **THEN** Task C 恢复调用 `DisplayService::present()`，UI 立即显示熄屏前的画面状态
