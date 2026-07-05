## Context

change 01 已落地工程骨架：`Cargo.toml` / `build.rs` / `sdkconfig.defaults` / `rust-toolchain.toml` / `.cargo/config.toml` / `partitions.csv` / `src/board_profile.rs`（`BoardProfile` 全部编译期常量）/ `ui/boot.slint` / `src/main.rs`（slint 占位画面阻塞主循环）。`src/hal/mod.rs` 为空 `mod.rs` 占位。

目标硬件 Waveshare ESP32-C6-Touch-LCD-1.54：ESP32-C6 RISC-V 单核 160MHz / 512KB SRAM / 16MB Flash / 无 PSRAM。外设按技术设计 §2 引脚常量映射：BAT_ADC=GPIO0、BACKLIGHT=GPIO6、BOOT=GPIO9、PA_CTRL=GPIO15、PLUS=GPIO18；LCD ST7789 240×240 SPI；Touch CST816 I2C；Mic ES7210 I2S；Speaker ES8311 + NS4150B PA I2S；射频 ESP-NOW + Wi-Fi 共用单 RF。

技术栈已锁定：Rust + esp-idf-svc (std)。本变更是 HAL/BSP 层，**不**实现 Service trait（DisplayService/InputService/PowerService/AudioService/NetworkService 签名见技术设计 §3.x，change 03-06 才实现）。

## Goals / Non-Goals

**Goals:**
- `src/hal/` 下 8 个驱动模块 + 1 个聚合 `Hal::init()` 一次性初始化全部外设，返回可用句柄
- 所有外设引脚 / 时钟 / I2C 地址 / I2S 采样率 / DMA 缓冲大小源自 `BoardProfile` 常量，不在驱动内硬编码
- `cargo build` 通过；烧录后 `Hal::init()` 不 panic，UART 日志逐行打印每个子模块 init OK
- 背光可调亮度并支持 `on/off`；按键 GPIO 中断（ISR）推送原始边沿事件到队列（`*GpioPress`/`*GpioRelease`），不做消抖 / 分类（分类由 change 04 InputService 在 Task C 上完成）
- ES7210 采集 I2S RX 与 ES8311 播放 I2S TX 通道就绪（DMA 缓冲空跑不产生业务音频），PA_CTRL 为纯 GPIO 开关（`pa_enable(on)` 仅拉高 / 拉低，无时序逻辑——软启动由 change 05 AudioService 负责）
- ESP-NOW 在发现信道 channel=1 初始化完成，可后续 add_peer（不发送任何业务包）
- 各驱动对外暴露的句柄为 `Send`（可跨 task 传递，配合 change 01 三 task 模型）

**Non-Goals:**
- 不实现任何 Service trait —— DisplayService/InputService/PowerService/AudioService/NetworkService 在 change 03-06
- 不实现 PTT 状态机 / 音量面板 UI / 熄屏策略 / 组队协议 —— change 07+
- 不实现 Opus 编解码 / 抖动缓冲 / 混音 —— change 05/10/11
- 不实现 ESP-NOW 包格式 / add_peer 实际写入 / 信道切换 —— change 06/08/09
- 不修改 `main.rs` 接入业务（本期 `main.rs` 可临时调用 `Hal::init()` 验证不 panic，但保留 slint 占位画面主循环）
- 不实现 OTA / 诊断上报 —— change 16/17
- 不引入单元测试（嵌入式硬件驱动，靠 on-device 烧录验收）

## Decisions

### D1：驱动封装风格 = `esp-idf-svc` 资源句柄的薄封装
每个驱动文件定义一个 `pub struct XxxDriver { ... }`，构造时持有 `esp_idf_svc::xxx::Xxx` 句柄（`SpiDriver` / `I2cDriver` / `LedcDriver` / `AdcDriver` / `I2sDriver` / `EspNow` 等），暴露 `init(peripherals, pins) -> Result<Self, HalError>` 与最小方法集。不引入第三方 HAL crate（如 `mipidsi`/`display-interface`），保持依赖最小、可控。备选：直接用 `mipeds4822790` 等 crate——版本漂移与 esp-idf-svc std 兼容性风险，排除。

### D2：SPI 引脚 = 技术设计 §2 + Waveshare 原理图
ST7789 SPI 引脚：SCLK=GPIO1 / MOSI=GPIO3 / DC=GPIO7 / RST=GPIO4 / CS=GPIO8 / BL=GPIO6（与 `BACKLIGHT_PIN` 一致，由 `backlight.rs` 独立控制）。SPI 时钟 40MHz（ST7789 上限 62MHz，40MHz 稳定）。SPI bus 由 `lcd.rs` 独占。备选：共享 SPI bus——本期无其他 SPI 设备，无需抽象 bus 共享。

### D3：CST816 触摸 = I2C + IRQ 引脚
CST816 I2C 地址 0x15，SDA=GPIO5 / SCL=GPIO6？——**冲突**：GPIO6 已为背光 PWM。重新核对 Waveshare ESP32-C6-Touch-LCD-1.54 原理图：触摸 I2C SDA=GPIO5 / SCL=GPIO7？需在实现时实际验证；本期在 `BoardProfile` 增 `TOUCH_SDA_PIN` / `TOUCH_SCL_PIN` / `TOUCH_IRQ_PIN` 常量并标注"以原理图为准"。备选：硬编码——违反 change 01 D5「常量集中」原则，排除。

> 注：`BoardProfile` 的常量扩展需在实现阶段追加；本变更 spec 不强制 change 01 文件结构不变，但仅追加常量不修改既有常量。

### D4：按键 = GPIO 中断（ISR）推送原始边沿事件到队列
BOOT(GPIO9) / PLUS(GPIO18) / PWR GPIO 配置为 `PinDriver::input` + 上下拉（以原理图为准）+ 双边沿中断。ISR 内直接将原始边沿事件（`BootGpioPress`/`BootGpioRelease`、`PlusGpioPress`/`PlusGpioRelease`、`PwrGpioPress`/`PwrGpioRelease`）推送到 FreeRTOS 队列。**不创建第 4 个 FreeRTOS task**——消抖 / 短触 / 长按分类由 change 04 InputService 在 Task C 上消费队列完成。事件 enum 在本文件定义（change 04 InputService 再映射到 `InputEvent`）。备选：纯轮询——CPU 浪费、响应延迟，排除；备选：BSP 内消抖 task——违反三 task 模型（D12），排除。

### D5：背光 PWM = LEDC channel0 + GPIO6
`esp_idf_svc::ledc::LedcDriver` + `LedcTimer` + `LedcChannel`，频率 5kHz / 8bit 分辨率（256 级，足够）。`set_brightness(v: u8)` 线性映射 0-100 → 0-255（≥1 以保证完全熄灭为 0、最亮为 255）。`off()` = duty 0；`on()` = 恢复上次非零 duty。备选：`ledc` 高速通道——ESP32-C6 仅低速通道，无需选择。

### D6：电池 ADC = ADC1 原始采样仅
GPIO0 = ADC1_CH0（ESP32-C6）。分压×3 → 满量程对应约 3.3V×3=9.9V，单节锂电 3.0-4.2V 对应 ADC 约 0.91V-1.27V → 原始值约 1130-1580（12bit）。BSP 仅提供 `read_raw_adc() -> u16`（单次 12bit 原始采样）。**不提供平滑 / 档位 / 百分比映射**——平滑（EWMA）与 4 档滞回映射由 change 04 PowerService 负责（D4 单一数据源）。不做"避开发射瞬时采样"调度（属 PowerService 业务）。

### D7：ES7210 采集 = I2C 配置 + I2S0 RX
ES7210 I2C 地址 0x40。I2S0 RX：采样率 16kHz（`OPUS_SAMPLE_RATE`）、16bit、单声道、DMA 缓冲 4 帧×320B（20ms 帧 = 320 sample × 2B）。I2S 引脚：BCLK=GPIO12 / WS=GPIO13 / DIN=GPIO14（以原理图为准，`BoardProfile` 追加常量）。`init()` 内：I2C 写 ES7210 寄存器（power up / set sample rate / unmute）→ 启动 I2S RX（不读数据，DMA 空跑）。备选：用 ES7210 datasheet 默认寄存器——需实测，先按 Waveshare 示例配置。

### D8：ES8311 + NS4150B 播放 = I2C 配置 + I2S0 TX + PA_CTRL GPIO15
ES8311 I2C 地址 0x18。I2S0 TX：与 ES7210 共用 I2S0？ESP32-C6 仅 1 个 I2S 控制器，RX/TX 可在同 controller 上全双工。但技术设计 §2.3 "不做真全双工"——采集与播放分时启用。本期 `init()` 同时初始化 RX/TX 通道但**不同时启动**，由后续 AudioService 控制 start/stop。PA_CTRL=GPIO15：`PinDriver::output`，初始 low；`pa_enable(on: bool)` 为**纯 GPIO 开关**——`on=true` 拉高、`on=false` 拉低，无延时 / 软启动时序（D6：软启动由 change 05 AudioService 负责：`pa_enable(true)` → wait 1-2 frames buffer → start I2S output；stop: output zero frames 1-2 frames → `pa_enable(false)` → stop I2S）。

### D9：ESP-NOW = `EspNow` 包装 + 发现信道
`esp_idf_svc::espnow::EspNow` 初始化 channel=1（`DISCOVERY_CHANNEL`）。本期不 add_peer、不注册回调、不发任何包。`Hal::init()` 内先 `EspWifi` 初始化（STA 模式）再 `EspNow::new()`。备选：直接 `EspNow::new` 无需 wifi——esp-idf-svc 要求先初始化 wifi，排除裸 EspNow。

### D10：`Hal::init` 顺序 = 外设依赖序
固定顺序（任一失败立即返回 `HalError`）：
1. 取 `peripherals`，pinout 拆分
2. `Backlight::init`（背光先亮，便于观察后续 init 进度）
3. `Lcd::init`（SPI + ST7789 reset 序列）
4. `Touch::init`（I2C + CST816 reset + IRQ 配置）
5. `Buttons::init`（GPIO9/18/PWR 中断 + 队列）
6. `Battery::init`（ADC1 CH0 校准）
7. `AudioIn::init`（I2C 配 ES7210 + I2S0 RX）
8. `AudioOut::init`（I2C 配 ES8311 + I2S0 TX + PA_CTRL low）
9. `Radio::init`（EspWifi + EspNow channel=1）
任一步 panic / Err → 返回 `HalError::XxxInitFailed(msg)`，UART error 日志。

### D11：错误类型 = 单一 `HalError` enum
`#[derive(Debug)] pub enum HalError { LcdInitFailed(String), TouchInitFailed(String), ... }`，每变体携带 `String` 上下文。`From<esp_idf_svc::error::EspError>` 等统一转换。不引入 `thiserror`（依赖最小化）。

## Risks / Trade-offs

- **[CST816 / ES7210 / ES8311 I2C 引脚与地址未在技术设计 §2 显式列出]** → 实现阶段查 Waveshare ESP32-C6-Touch-LCD-1.54 原理图与示例 firmware 确认；`BoardProfile` 追加 `TOUCH_SDA_PIN` / `TOUCH_SCL_PIN` / `TOUCH_IRQ_PIN` / `I2S_BCLK_PIN` / `I2S_WS_PIN` / `I2S_DIN_PIN` / `I2S_DOUT_PIN` 常量，spec 验收时确认值与原理图一致
- **[esp-idf-svc I2S std API 在 ESP32-C6 上的成熟度]** → 若 `I2sDriver` RX/TX 全双工 API 不支持，则采集与播放改用两个独立 I2S 通道或分时复用单 I2S；本期 `init()` 仅启动 DMA 空跑，业务音频在 change 05 集成时验证
- **[ES7210 / ES8311 寄存器配置序列]** → 先按 Waveshare 官方示例 firmware 的寄存器序列；若音质异常在 change 05 调参，本期不要求音质
- **[GPIO6 同时用于背光 PWM 与（可能的）CST816 SCL]** → 已在 D3 标注冲突，实现阶段核对原理图后修正 `TOUCH_SCL_PIN`
- **[ADC1 CH0 与 Wi-Fi 共存引起的射频干扰]** → 已知问题，PowerService 业务层调度采样避开发射窗口；本 BSP 层仅提供 `read_raw_adc()` API，不做调度
- **[GPIO ISR → 队列延迟]** → ISR 仅做 `queue.send_from_isr()`，延迟 < 1ms；消费方在 change 04 InputService Task C 上处理
- **[`Hal` 句柄跨 task Send]** → `esp-idf-svc` 资源句柄多数 `Send` 但非 `Sync`；`Hal` 设计为可被 move 到目标 task，不跨核共享（ESP32-C6 单核，无核间共享问题）
- **[`Hal::init` 部分失败后的资源清理]** → esp-idf-svc 资源 RAII，`Hal` 部分构造的中间变量 drop 自动释放；失败时 UART 日志标注失败模块

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译
3. `cargo espflash flash --port <port> --release` 烧录
4. 上电观察：背光点亮 → LCD 仍显示 change 01 的 "Intercom Boot OK"（slint 主循环未改）→ UART 日志逐行打印 Backlight/Lcd/Touch/Buttons(含 PWR)/Battery/AudioIn/AudioOut/Radio init OK
5. 触摸屏 / 按键 / ADC 暂无可见业务响应（业务在 change 04+）；仅验证 init 不 panic；按键 ISR 事件队列已就绪但无消费方

回滚：`git revert <commit>`，回到 change 01 的空 `src/hal/mod.rs`。

## Open Questions

- CST816 / ES7210 / ES8311 的 I2C 引脚与 I2S 引脚需在实现阶段以 Waveshare ESP32-C6-Touch-LCD-1.54 原理图为准确认；技术设计 §2 未显式列出这些引脚常量，本变更在 `BoardProfile` 追加时标注来源。
- ES7210 / ES8311 的寄存器配置序列先按 Waveshare 示例 firmware，音质调参延后到 change 05。
