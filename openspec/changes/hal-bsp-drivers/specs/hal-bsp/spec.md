## ADDED Requirements

### Requirement: HAL 聚合初始化
项目 SHALL 提供 `src/hal/mod.rs` 定义 `pub struct Hal` 与 `pub fn init(peripherals: Peripherals) -> Result<Hal, HalError>`，按固定顺序初始化全部外设（背光 → LCD → 触摸 → 按键 → 电池 → 采集 → 播放 → 射频）。任一子模块初始化失败 SHALL 立即返回 `HalError` 并通过 UART 日志标注失败模块，不继续后续初始化。

#### Scenario: 全部外设初始化成功
- **WHEN** 在目标硬件上电后调用 `Hal::init(peripherals)`
- **THEN** 返回 `Ok(Hal)`，UART 日志按顺序打印 8 个子模块（Backlight / Lcd / Touch / Buttons[含 PWR] / Battery / AudioIn / AudioOut / Radio）的 init OK 行，`Hal` 持有全部驱动句柄

#### Scenario: 单一外设初始化失败
- **WHEN** `Touch::init` 因 I2C 应答失败返回 Err
- **THEN** `Hal::init` 立即返回 `Err(HalError::TouchInitFailed(...))`，不再初始化 Battery/AudioIn/AudioOut/Radio，UART 日志包含失败模块名与错误上下文

### Requirement: ST7789 LCD SPI 驱动
`src/hal/lcd.rs` SHALL 提供 `LcdDriver`，通过 SPI（SCLK/MOSI/DC/RST/CS 引脚源自 `BoardProfile`）初始化 ST7789 控制器，提供帧缓冲写入接口（240×240 RGB565 或 RGB888，以 `BoardProfile::LCD_W`/`LCD_H` 为分辨率上限）。SPI 时钟 SHALL 不超过 ST7789 datasheet 上限（40MHz 用于稳定裕度）。

#### Scenario: LCD 初始化后帧缓冲可写入
- **WHEN** `LcdDriver::init` 成功后调用帧写入方法提交一帧全屏红色数据
- **THEN** LCD 显示全屏红色，无花屏 / 撕裂

#### Scenario: 引脚常量源自 BoardProfile
- **WHEN** 检查 `lcd.rs` 的 SPI 引脚字面量
- **THEN** 全部引用 `BoardProfile::LCD_SPI_SCLK_PIN` 等常量，不出现裸数字字面量（pinout 校验时与原理图对照）

### Requirement: CST816 触摸 I2C 驱动
`src/hal/touch.rs` SHALL 提供 `TouchDriver`，通过 I2C（地址 0x15，SDA/SCL/IRQ 引脚源自 `BoardProfile`）初始化 CST816，配置 IRQ 中断，提供 `read_event() -> TouchEvent` 接口。`TouchEvent` SHALL 至少包含 `Down(x,y)` / `Up(x,y)` / `Swipe(delta)` 三类。

#### Scenario: 触摸产生中断事件
- **WHEN** 用户在 CST816 触摸屏上按下并抬起
- **THEN** `read_event` 依次返回 `Down(x,y)` 与 `Up(x,y)`，坐标在 0..240 范围内

#### Scenario: 触摸引脚不与背光冲突
- **WHEN** 检查 `BoardProfile` 的 `TOUCH_SCL_PIN` / `TOUCH_SDA_PIN`
- **THEN** 二者均不等于 `BACKLIGHT_PIN`(=6)，值与 Waveshare ESP32-C6-Touch-LCD-1.54 原理图一致

### Requirement: 按键 GPIO 原始边沿事件驱动
`src/hal/buttons.rs` SHALL 提供 `ButtonsDriver`，对 BOOT(GPIO9) + PLUS(GPIO18) + PWR GPIO 配置双边沿输入中断。GPIO ISR SHALL 将原始边沿事件推送到 FreeRTOS 队列，不做消抖 / 短触 / 长按分类（分类由 change 04 InputService 在 Task C 上完成）。SHALL 输出六类原始事件：`BootGpioPress` / `BootGpioRelease` / `PlusGpioPress` / `PlusGpioRelease` / `PwrGpioPress` / `PwrGpioRelease`。SHALL NOT 创建第 4 个 FreeRTOS task（D12：仅 ISR + 队列，消费方在 change 04）。

#### Scenario: BOOT 按下产生原始边沿事件
- **WHEN** 用户按下 BOOT 按键（GPIO9 下降沿）
- **THEN** 队列收到 `BootGpioPress` 事件；松开时收到 `BootGpioRelease` 事件

#### Scenario: PLUS 按下产生原始边沿事件
- **WHEN** 用户按下 PLUS 按键（GPIO18 下降沿）
- **THEN** 队列收到 `PlusGpioPress` 事件；松开时收到 `PlusGpioRelease` 事件

#### Scenario: PWR 按下产生原始边沿事件
- **WHEN** 用户按下 PWR 按键
- **THEN** 队列收到 `PwrGpioPress` 事件；松开时收到 `PwrGpioRelease` 事件

#### Scenario: BSP 不做消抖分类
- **WHEN** 检查 `buttons.rs` 代码
- **THEN** 不存在 delay / timer / 短触长按阈值逻辑；仅 ISR → 队列推送原始边沿

### Requirement: 背光 LEDC PWM 驱动
`src/hal/backlight.rs` SHALL 提供 `BacklightDriver`，通过 GPIO6 LEDC PWM（频率 5kHz / 8bit 分辨率）控制背光。SHALL 提供 `set_brightness(v: u8)`（0-100 映射到 0-255 duty）、`on()`（恢复上次非零 duty）、`off()`（duty=0）。

#### Scenario: 设置中间亮度
- **WHEN** 调用 `set_brightness(50)`
- **THEN** LEDC duty 寄存器值约为 128，LCD 背光明显变暗但可见

#### Scenario: off 后 on 恢复亮度
- **WHEN** 当前亮度 80，调用 `off()` 后再调用 `on()`
- **THEN** 亮度恢复为 80，不回到默认 100

### Requirement: 电池 ADC 原始采样驱动
`src/hal/battery.rs` SHALL 提供 `BatteryDriver`，通过 ADC1 CH0（GPIO0，×3 分压）采样。SHALL 仅提供 `read_raw_adc() -> u16`（原始 12bit 值）。SHALL NOT 提供平滑 / 档位 / 百分比映射（D4：映射由 change 04 PowerService 作为单一数据源负责）。SHALL NOT 实现"避开发射瞬时采样"调度（属 PowerService 业务）。

#### Scenario: 原始采样返回 12bit 值
- **WHEN** 接 4.2V 电源调用 `read_raw_adc()`
- **THEN** 返回值在 1130-1580 范围内（×3 分压后 12bit 量化）

#### Scenario: BSP 不做档位映射
- **WHEN** 检查 `battery.rs` 代码
- **THEN** 不存在 `step()` / `smoothed()` / 电压阈值 / 档位映射逻辑

### Requirement: ES7210 采集 I2S 驱动
`src/hal/audio_in.rs` SHALL 提供 `AudioInDriver`，通过 I2C（地址 0x40）配置 ES7210 寄存器（power up / 16kHz / unmute），通过 I2S0 RX（BCLK/WS/DIN 引脚源自 `BoardProfile`，采样率 `OPUS_SAMPLE_RATE`=16000 / 16bit / 单声道 / DMA 缓冲）启动采集通道。`init()` SHALL NOT 实际读取业务音频数据（DMA 空跑即可），业务采集在 change 05 AudioService。

#### Scenario: 采集通道初始化后 DMA 空跑不 panic
- **WHEN** `AudioInDriver::init` 成功后等待 100ms
- **THEN** 无 panic / 无 I2S 错误中断，UART 无错误日志

### Requirement: ES8311 + NS4150B 播放 I2S 驱动
`src/hal/audio_out.rs` SHALL 提供 `AudioOutDriver`，通过 I2C（地址 0x18）配置 ES8311 寄存器，通过 I2S0 TX（与采集共用 I2S0 controller 的 TX 通道，引脚 BCLK/WS/DOUT 源自 `BoardProfile`，采样率 16kHz / 16bit / 单声道 / DMA 缓冲）启动播放通道，并持有 GPIO15 `PA_CTRL` 句柄。`pa_enable(on: bool)` SHALL 为纯 GPIO 开关：`on=true` 拉高 PA_CTRL、`on=false` 拉低 PA_CTRL，无延时 / 软启动时序（D6：软启动序列由 change 05 AudioService 负责）。`init()` SHALL 启动 TX 通道但 PA 保持 low（不产生爆音）。

#### Scenario: PA 开关为纯 GPIO 操作
- **WHEN** 调用 `pa_enable(true)` 后立即调用 `pa_enable(false)`
- **THEN** PA_CTRL 引脚分别被拉高再拉低，两次操作间无 delay / 计时逻辑

#### Scenario: PA 关闭立即静音
- **WHEN** 播放中调用 `pa_enable(false)`
- **THEN** PA_CTRL 立即拉低，扬声器立即静音

### Requirement: ESP-NOW 射频初始化
`src/hal/radio.rs` SHALL 提供 `RadioDriver`，先初始化 `EspWifi`（STA 模式）再构造 `EspNow`，初始信道 = `BoardProfile::DISCOVERY_CHANNEL`(=1)。SHALL NOT add_peer / 注册接收回调 / 发送任何业务包（业务在 change 06 NetworkService）。`Hal::init` 完成 SHALL 持有 `EspNow` 句柄供后续 add_peer。

#### Scenario: ESP-NOW 初始化在发现信道
- **WHEN** `RadioDriver::init` 成功后查询当前信道
- **THEN** 信道号为 1

### Requirement: 错误类型与日志
`src/hal/mod.rs` SHALL 定义 `pub enum HalError`，每变体对应一个子模块初始化失败（`LcdInitFailed` / `TouchInitFailed` / `ButtonsInitFailed` / `BacklightInitFailed` / `BatteryInitFailed` / `AudioInInitFailed` / `AudioOutInitFailed` / `RadioInitFailed`），携带 `String` 上下文。`Hal::init` 失败 SHALL 通过 `log::error!` 输出失败模块名与上下文。

#### Scenario: 失败日志可定位
- **WHEN** `AudioInInitFailed` 返回
- **THEN** UART 日志包含 "AudioIn init failed: <上下文>"，可定位失败原因

### Requirement: 引脚常量集中
`BoardProfile` SHALL 追加本变更所需的全部引脚常量：`LCD_SPI_SCLK_PIN` / `LCD_SPI_MOSI_PIN` / `LCD_DC_PIN` / `LCD_RST_PIN` / `LCD_CS_PIN` / `TOUCH_SDA_PIN` / `TOUCH_SCL_PIN` / `TOUCH_IRQ_PIN` / `I2S_BCLK_PIN` / `I2S_WS_PIN` / `I2S_DIN_PIN` / `I2S_DOUT_PIN` / `I2C_SDA_PIN` / `I2C_SCL_PIN` / `PWR_BTN_PIN`（若触摸与音频共享 I2C bus 则用同一对常量）。所有驱动文件 SHALL 引用这些常量，不出现裸数字字面量。

#### Scenario: 引脚常量与原理图一致
- **WHEN** 检查 `BoardProfile` 追加的引脚常量
- **THEN** 每个值与 Waveshare ESP32-C6-Touch-LCD-1.54 原理图对应引脚一致，无冲突（同一 GPIO 不被两个外设占用，除非设计上共享如 I2C bus）

### Requirement: 驱动句柄 Send
`Hal` 与各 `XxxDriver` SHALL 为 `Send`（可在 FreeRTOS task 间 move），不要求 `Sync`（ESP32-C6 单核，无需跨核共享）。

#### Scenario: 句柄 move 到 task
- **WHEN** change 04+ 在 `Hal::init` 后将 `Hal` move 到 Task C
- **THEN** 编译通过，无 `Send` trait bound 错误
