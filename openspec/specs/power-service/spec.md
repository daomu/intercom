## ADDED Requirements

### Requirement: PowerService trait 定义
项目 SHALL 在 `src/services/power.rs` 中定义 `PowerService` trait，包含以下方法签名：`fn battery_level(&self) -> u8`、`fn battery_step(&self) -> u8`、`fn enter_standby(&self)`、`fn wakeup(&self)`、`fn reset_reason(&self) -> ResetReason`、`fn abnormal_boot_count(&self) -> u32`。该 trait SHALL 继承 `Send + Sync`。

#### Scenario: trait 可被引用
- **WHEN** 在 `src/apps/` 或 `src/intercom/` 中 `use crate::services::power::PowerService`
- **THEN** trait 及其方法签名在编译期可见，可构造 `Box<dyn PowerService>`

### Requirement: ResetReason 枚举定义
项目 SHALL 定义 `ResetReason` 枚举，包含以下变体：`PowerOn`、`Brownout`、`Wdt`、`Panic`、`Unknown`。

#### Scenario: 枚举可匹配
- **WHEN** 在启动流程中 `match power_service.reset_reason() { ResetReason::PowerOn => ..., ResetReason::Panic => ..., _ => ... }`
- **THEN** 编译通过，所有变体可见

### Requirement: 电池电量读取
`battery_level()` SHALL 返回平滑后的电池电量百分比，范围 0-100。电量估算 SHALL 基于 GPIO0 ADC 原始值（分压 ×3）转换为电压后映射到百分比，并经过指数移动平均平滑处理。

#### Scenario: 正常电量读取
- **WHEN** 电池电压约 3.7V，ADC 采样稳定
- **THEN** `battery_level()` 返回 50-100 范围内的稳定值，不跳变

#### Scenario: 低电量
- **WHEN** 电池电压约 3.3V
- **THEN** `battery_level()` 返回 0-25 范围内的值

### Requirement: 电池 4 档图标映射（单一数据源）
`battery_step()` SHALL 是电池档位映射的唯一数据源（BSP 仅提供 raw ADC 值，不做档位/百分比映射）。映射流程：raw ADC → 电压（除以 `BoardProfile::BAT_ADC_DIVIDER`，即 ×3 分压还原）→ 4 档图标，阈值如下（带 ±0.1V 滞回）：
- < 3.4V → 1 bar（critical）
- 3.4–3.6V → 2 bars（low）
- 3.6–3.9V → 3 bars（medium）
- > 3.9V → 4 bars（full）

档位切换 SHALL 实现 ±0.1V 滞回带，避免在阈值边界处频繁跳变。

#### Scenario: 满电档位
- **WHEN** 电池电压约 4.0V
- **THEN** `battery_step()` 返回 4

#### Scenario: 低电档位
- **WHEN** 电池电压约 3.3V
- **THEN** `battery_step()` 返回 1

#### Scenario: 滞回防跳变
- **WHEN** 电压在 3.6V 附近波动（3.59V → 3.61V → 3.59V）
- **THEN** `battery_step()` 不在 2 和 3 之间频繁切换，需穿越 ±0.1V 滞回带后才切换

#### Scenario: BSP 仅提供 raw ADC
- **WHEN** PowerService 调用 BSP 的 ADC 读取接口
- **THEN** BSP 返回 raw ADC 数值（u16），不进行任何电压转换或档位映射

### Requirement: 采样规避发射/大音量干扰
电池 ADC 采样 SHALL 在音频发射或大音量播放期间暂停，避免瞬态干扰。采样周期 SHALL 为 2 秒。

#### Scenario: 发射期间暂停采样
- **WHEN** 设备正在 PTT 发言（音频采集 + ESP-NOW 发射中）
- **THEN** `battery_level()` 返回上一次有效平滑值，不进行新 ADC 采样

#### Scenario: 空闲期间恢复采样
- **WHEN** 音频发射结束 2 秒后
- **THEN** ADC 恢复周期性采样，`battery_level()` 更新为最新值

### Requirement: 进入待机
`enter_standby()` SHALL 执行以下操作：调用 `DisplayService::screen_off()` 熄灭屏幕、关闭 PA 控制引脚（GPIO15）。SHALL NOT 关闭 Wi-Fi/ESP-NOW，以保持对讲模式下的收发能力。

#### Scenario: 对讲模式待机
- **WHEN** 设备处于已组网待机状态，调用 `enter_standby()`
- **THEN** 屏幕熄灭、PA 关闭，但 Wi-Fi/ESP-NOW 仍在运行，可接收语音包

#### Scenario: 非对讲模式待机
- **WHEN** 设备未组网且空闲，调用 `enter_standby()`
- **THEN** 屏幕熄灭、PA 关闭，进入低功耗状态

### Requirement: 唤醒
`wakeup()` SHALL 恢复屏幕亮屏（调用 `DisplayService::screen_on()`）并恢复 PA 控制引脚为可用状态。进入 Listening 前 SHALL 软启动 PA 以避免爆音。

#### Scenario: 从待机唤醒
- **WHEN** 在待机状态下调用 `wakeup()`
- **THEN** 屏幕亮起，PA 恢复供电，后续音频播放无爆音

### Requirement: 启动原因诊断
`reset_reason()` SHALL 读取 ESP-IDF RTC reset cause 并映射到 `ResetReason` 枚举：上电复位 → `PowerOn`、掉电复位 → `Brownout`、看门狗复位 → `Wdt`、panic 重启 → `Panic`、其他 → `Unknown`。

#### Scenario: 正常上电
- **WHEN** 设备通过 USB 供电首次上电
- **THEN** `reset_reason()` 返回 `ResetReason::PowerOn`

#### Scenario: 看门狗复位
- **WHEN** 设备因看门狗超时复位后重启
- **THEN** `reset_reason()` 返回 `ResetReason::Wdt`

### Requirement: 异常启动计数
`abnormal_boot_count()` SHALL 返回从 NVS（通过 `StorageService::load_diag()`）读取的异常启动计数。该计数 SHALL 在每次 panic/看门狗/brownout 复位时由 `StorageService::inc_abnormal_boot()` 递增（该递增逻辑在后续变更中实现，本变更仅提供读取接口）。

#### Scenario: 首次启动计数为零
- **WHEN** 设备首次烧录后启动
- **THEN** `abnormal_boot_count()` 返回 0

#### Scenario: 多次异常后计数递增
- **WHEN** 设备经历了 2 次 panic 重启
- **THEN** `abnormal_boot_count()` 返回 2（依赖 `StorageService::inc_abnormal_boot` 已实现）

### Requirement: 模块注册
`src/services/mod.rs` SHALL 包含 `pub mod power;` 以注册 power 子模块。

#### Scenario: 模块可见
- **WHEN** 在 `src/main.rs` 或其他模块中 `use crate::services::power::PowerService`
- **THEN** 路径解析成功，编译通过
