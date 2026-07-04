## Why

17 个变更提案的第 2 个。change 01 已落地工程骨架与 `BoardProfile` 编译期常量，但 `src/hal/` 仍为空 `mod.rs` 占位，没有任何硬件驱动。后续所有 Service（Display/Input/Power/Audio/Network，change 03-06）与对讲业务（change 08+）都需要底层外设句柄与初始化代码。本变更为 ST7789 LCD / CST816 触摸 / BOOT+PLUS 按键 / 背光 PWM / 电池 ADC / ES7210 采集 / ES8311+NS4150B 播放 / ESP-NOW 射频提供可直接 `init()` 的 BSP 驱动模块，使硬件外设一次性就绪。

## What Changes

- 新增 `src/hal/lcd.rs`：ST7789 SPI 驱动封装（240×240，SPI 时钟 / DC / RST / CS 引脚初始化，提供 `init()` 返回 `Display` 句柄，支持帧缓冲写入）
- 新增 `src/hal/touch.rs`：CST816 I2C 触摸驱动封装（初始化 + 中断引脚配置 + 事件读取 `Down/Up/Swipe`）
- 新增 `src/hal/buttons.rs`：BOOT(GPIO9) + PLUS(GPIO18) + PWR GPIO 输入中断驱动，ISR 将原始边沿事件（`BootGpioPress`/`BootGpioRelease`、`PlusGpioPress`/`PlusGpioRelease`、`PwrGpioPress`/`PwrGpioRelease`）推送到队列，不做消抖 / 短触 / 长按分类（分类由 change 04 InputService 负责）
- 新增 `src/hal/backlight.rs`：GPIO6 LEDC PWM 背光驱动（`set_brightness(0-100)` / `on()` / `off()`）
- 新增 `src/hal/battery.rs`：GPIO0 ADC1 通道驱动（×3 分压，仅提供 `read_raw_adc() -> u16` 原始采样，不做档位 / 百分比映射——映射由 change 04 PowerService 负责）
- 新增 `src/hal/audio_in.rs`：ES7210 I2S 采集初始化（I2C 配置寄存器 + I2S RX 通道 + DMA 缓冲，单声道 16kHz）
- 新增 `src/hal/audio_out.rs`：ES8311 I2S 播放初始化 + GPIO15 PA_CTRL 纯 GPIO 开关（`pa_enable(on: bool)` 仅拉高 / 拉低 PA_CTRL 引脚，无延时 / 软启动时序——软启动由 change 05 AudioService 负责）
- 新增 `src/hal/radio.rs`：ESP-NOW + Wi-Fi 共用单射频初始化（`EspWifi` + `EspNow` 句柄持有，初始化发现信道 channel=1）
- 新增 `src/hal/mod.rs`：聚合上述子模块，导出 `Hal` 结构体 `init(peripherals) -> Result<Hal, HalError>` 一次性初始化全部外设，返回各句柄
- 不实现任何 Service trait（DisplayService/InputService/AudioService/PowerService）——Service trait 在 change 03-06
- 不实现业务逻辑（PTT 状态机 / 音量面板 / 熄屏策略 / 组队协议）——这些在 change 07+

## Capabilities

### New Capabilities
- `hal-bsp`: 硬件抽象层 / 板级支持包，封装 ESP32-C6 全部外设初始化与底层驱动句柄，对上层 Service 提供可直接持有的资源；不包含业务行为

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 hal-bsp -->

## Impact

- **代码**：`src/hal/` 由空 `mod.rs` 升级为 8 个驱动文件 + 1 个聚合 `mod.rs`；不修改 change 01 的 `board_profile.rs` 与 `main.rs`（本期 `main.rs` 仅在内部调用 `Hal::init` 校验编译，不接入业务）
- **依赖**：变更依赖 = `01`（工程骨架与 `BoardProfile` 常量）。`esp-idf-svc` 已在 change 01 声明，本期实际调用其 `spi`/`i2c`/`ledc`/`adc`/`gpio`/`i2s`/`wifi`/`espnow` 模块；无新增 crate 依赖
- **后续变更**：change 03-06 各 Service 在本 BSP 句柄上实现 trait；change 08+ 对讲业务通过 Service 间接使用；本变更不向上暴露 trait，只暴露资源句柄
- **硬件**：一次性初始化全部外设，烧录后串口日志应打印每个子模块 init 成功；本期不要求屏幕显示业务画面（仍为 change 01 的 slint 占位画面，由 `Hal::init` 后续接管 LCD 句柄再渲染——但本期 `main.rs` 暂不切走 slint 主循环，仅验证 init 不 panic）
- **无破坏性变更**：change 01 的工程骨架与构建配置不变
