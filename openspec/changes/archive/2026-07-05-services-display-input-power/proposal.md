## Why

17 个变更提案的第 4 个。change 02 已完成 BSP 驱动层（LCD/Touch/Buttons/ADC/Backlight 初始化），但上层没有 Display/Input/Power 三个 Service 的 trait 定义与实现。后续 change 07（app-shell-settings）、change 10（voice-ptt）、change 13（app-ui）、change 15（power-screen-management）全部依赖这三个 Service 的稳定接口。本期在此gap 中补齐三个 Service 的 trait + impl + 相关枚举，使设备具备"屏幕亮度控制 + 熄屏/亮屏 + 触摸/按键事件分发 + 电量读取 + 待机/唤醒 + 启动原因诊断"能力。

## What Changes

- 新增 `src/services/display.rs`：定义 `DisplayService` trait（`set_brightness` / `screen_on` / `screen_off` / `is_screen_on` / `present`）与基于 LEDC PWM 背光 + slint 渲染后端的实现；定义 `DrawCmd` 枚举封装 slint view 提交
- 新增 `src/services/input.rs`：定义 `InputService` trait（`on_event` 回调注册）与基于 CST816 触摸 + BOOT/PLUS/PWR 按键驱动的实现；定义 `InputEvent` 枚举（`BootPress { screen_was_off: bool }` / `BootRelease` / `BootShortTap` / `PlusShortPress` / `PlusLongPress` / `Touch(TouchEvent)` / `PowerShortPress`）与 `TouchEvent` 枚举（`Down(u16,u16)` / `Up(u16,u16)` / `Swipe(i8)`）；InputService 拥有全部按键阈值分类逻辑（BSP 仅提供 raw GPIO 边沿事件）
- 新增 `src/services/power.rs`：定义 `PowerService` trait（`battery_level` / `battery_step` / `enter_standby` / `wakeup` / `reset_reason` / `abnormal_boot_count`）与基于 ADC + RTC + NVS 诊断的实现；定义 `ResetReason` 枚举（`PowerOn` / `Brownout` / `Wdt` / `Panic` / `Unknown`）；PowerService 拥有完整电池映射（raw ADC → 电压 → 4 档图标，BSP 仅提供 raw ADC）
- BOOT 按键 PTT 阈值：按下 >= 50ms 视为 PTT（`BootPress`），< 50ms 释放视为 `BootShortTap`（唤醒屏幕）
- PLUS 按键：短按 = `PlusShortPress`（呼出音量面板），长按 = `PlusLongPress`（静音 toggle）
- PWR 按键：短按 = `PowerShortPress`（亮/熄屏 toggle），长按 = 硬件关机（不经软件）
- 触摸熄屏首触：仅唤醒屏幕，不产生 `Touch` 事件
- 电量 ADC：GPIO0 分压 ×3，平滑 + 滞回，避开发射/大音量瞬时采样；4 档图标映射基于电压阈值（<3.4/3.6/3.9V + ±0.1V 滞回），PowerService 为单一数据源
- 定义 `AppContext` 结构（持有 `&StorageService` / `&DisplayService` / `&PowerService` / `&Settings`）与 `App::on_enter` trait 签名（前向引用，change 07 消费）
- 在 `src/services/mod.rs` 注册 `display` / `input` / `power` 子模块

## Capabilities

### New Capabilities
- `display-service`: 屏幕背光 PWM 调光、亮屏/熄屏状态管理、绘制指令提交（slint 后端或直接 framebuffer）
- `input-service`: CST816 触摸事件（down/up/swipe）+ BOOT/PLUS/PWR 按键事件分发，含 PTT 50ms 阈值与短按/长按区分
- `power-service`: 电池电量 ADC 读取与平滑、4 档图标映射、待机/唤醒控制、启动原因诊断、异常启动计数

### Modified Capabilities
<!-- 无既有 spec 被修改 -->

## Impact

- **代码**：新增 `src/services/display.rs`、`src/services/input.rs`、`src/services/power.rs`；修改 `src/services/mod.rs` 注册三个子模块
- **依赖**：依赖 change 02 的 BSP 驱动（`hal::bsp` 暴露的 LCD/Touch/Button/ADC/Backlight 句柄，仅提供原始 ADC 与 raw GPIO 边沿事件，不做电池档位映射或按键去抖分类）；依赖 change 03 的 `StorageService` trait（用于 `abnormal_boot_count` 读取 NVS 诊断信息）；依赖 `BoardProfile` 常量（引脚号、默认亮度、默认熄屏时间、`BAT_ADC_DIVIDER`）
- **后续变更**：change 07（app-shell-settings）依赖 InputService 事件分发与 DisplayService 画面提交；change 10（voice-ptt）依赖 BootPress/BootRelease 事件；change 13（app-ui）依赖 DisplayService::present 与 TouchEvent；change 15（power-screen-management）依赖 PowerService::enter_standby/wakeup 与 DisplayService::screen_on/off
- **运行时行为**：设备获得屏幕亮度控制、输入事件分发、电量读取与待机能力；不影响音频链路（change 05）与网络（change 06）
