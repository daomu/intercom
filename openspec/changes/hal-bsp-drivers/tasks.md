## 1. BoardProfile 常量扩展

- [ ] 1.1 查 Waveshare ESP32-C6-Touch-LCD-1.54 原理图与示例 firmware，确认 ST7789 SPI 引脚（SCLK/MOSI/DC/RST/CS）、CST816 I2C 引脚与地址（0x15）、ES7210 I2C 地址（0x40）与 I2S 引脚、ES8311 I2C 地址（0x18）与 I2S 引脚，记录到 design.md Open Questions 收尾
- [ ] 1.2 在 `src/board_profile.rs` 追加常量：`LCD_SPI_SCLK_PIN` / `LCD_SPI_MOSI_PIN` / `LCD_DC_PIN` / `LCD_RST_PIN` / `LCD_CS_PIN` / `TOUCH_SDA_PIN` / `TOUCH_SCL_PIN` / `TOUCH_IRQ_PIN` / `I2S_BCLK_PIN` / `I2S_WS_PIN` / `I2S_DIN_PIN` / `I2S_DOUT_PIN` / `I2C_SDA_PIN` / `I2C_SCL_PIN` / `PWR_BTN_PIN`（共享 I2C 则同一对），值以 1.1 的原理图为准
- [ ] 1.3 确认追加常量不与既有 `BACKLIGHT_PIN`=6 / `BOOT_BTN_PIN`=9 / `PLUS_BTN_PIN`=18 / `BAT_ADC_PIN`=0 / `PA_CTRL_PIN`=15 冲突；若 `TOUCH_SCL_PIN` 与 `BACKLIGHT_PIN` 冲突则回原理图复核并修正

## 2. HalError 与聚合结构

- [ ] 2.1 在 `src/hal/mod.rs` 定义 `#[derive(Debug)] pub enum HalError`，变体：`BacklightInitFailed(String)` / `LcdInitFailed(String)` / `TouchInitFailed(String)` / `ButtonsInitFailed(String)` / `BatteryInitFailed(String)` / `AudioInInitFailed(String)` / `AudioOutInitFailed(String)` / `RadioInitFailed(String)`，实现 `From<EspError>` 与 `Display`
- [ ] 2.2 定义 `pub struct Hal { pub lcd: LcdDriver, pub touch: TouchDriver, pub buttons: ButtonsDriver, pub backlight: BacklightDriver, pub battery: BatteryDriver, pub audio_in: AudioInDriver, pub audio_out: AudioOutDriver, pub radio: RadioDriver }`
- [ ] 2.3 实现 `pub fn init(peripherals: Peripherals) -> Result<Hal, HalError>`，按 D10 顺序调用各子模块 `init`，任一失败立即 `return Err`，每步成功 `info!` 日志

## 3. 背光驱动

- [ ] 3.1 新增 `src/hal/backlight.rs`：`BacklightDriver { driver: LedcDriver }`，`init(peripherals, pin=BoardProfile::BACKLIGHT_PIN) -> Result<Self, HalError>`，配置 LedcTimer 频率 5kHz / 8bit + LedcChannel
- [ ] 3.2 实现 `set_brightness(v: u8)`：v∈0..=100 线性映射到 duty 0..=255，调用 `set_duty`；记录上次非零 duty
- [ ] 3.3 实现 `on()` / `off()`：`off` 置 duty 0；`on` 恢复上次非零 duty（若无则默认 `DEFAULT_BRIGHTNESS`）

## 4. LCD 驱动

- [ ] 4.1 新增 `src/hal/lcd.rs`：`LcdDriver { spi: SpiDriver, ... }`，`init` 内构造 `SpiDriver`（SCLK/MOSI/DC/RST/CS 引脚来自 BoardProfile，时钟 40MHz）
- [ ] 4.2 实现 ST7789 reset 序列（RST 拉低 10ms → 拉高 120ms）+ 初始化命令序列（sleep out / pixel format / 旋转方向 / display on），命令序列参考 Waveshare 示例
- [ ] 4.3 实现帧写入接口 `present(fb: &[u8])`：通过 SPI DMA 推送 240×240 像素（RGB565 或 RGB888，以实现为准）

## 5. 触摸驱动

- [ ] 5.1 新增 `src/hal/touch.rs`：`TouchDriver { i2c: I2cDriver, irq: PinDriver, ... }`，`init` 配置 I2C（地址 0x15，SDA/SCL 来自 BoardProfile）+ CST816 reset + IRQ 下降沿中断
- [ ] 5.2 实现 `read_event() -> TouchEvent`：读 CST816 状态寄存器，解析 `Down(x,y)` / `Up(x,y)` / `Swipe(delta)`；`TouchEvent` enum 在本文件定义（change 04 InputService 再映射到 `InputEvent`）
- [ ] 5.3 验证坐标范围 0..240

## 6. 按键驱动

- [ ] 6.1 新增 `src/hal/buttons.rs`：`ButtonsDriver { boot_pin, plus_pin, pwr_pin, queue: Sender<GpioEdgeEvent> }`，`init` 配置 GPIO9 / GPIO18 / PWR GPIO 为输入 + 双边沿中断（上下拉以原理图为准）+ 创建 FreeRTOS 队列（不创建 task——D12）
- [ ] 6.2 ISR 逻辑：双边沿中断回调内直接调用 `queue.send_from_isr()` 推送原始边沿事件，不做 delay / 消抖 / 阈值判断
- [ ] 6.3 定义 `GpioEdgeEvent` enum：`BootGpioPress` / `BootGpioRelease` / `PlusGpioPress` / `PlusGpioRelease` / `PwrGpioPress` / `PwrGpioRelease`，通过队列推送（消费方在 change 04 InputService Task C）
- [ ] 6.4 确认 PWR GPIO 引脚号（查 Waveshare ESP32-C6-Touch-LCD-1.54 原理图），追加到 `BoardProfile` 常量

## 7. 电池驱动

- [ ] 7.1 新增 `src/hal/battery.rs`：`BatteryDriver { adc: AdcDriver, ... }`，`init` 配置 ADC1 CH0（GPIO0 = `BAT_ADC_PIN`），12bit 采样
- [ ] 7.2 实现 `read_raw_adc() -> u16`：单次采样返回原始 12bit 值（D4：仅此一个 API，不提供 `smoothed()` / `step()`——映射由 change 04 PowerService 负责）

## 8. ES7210 采集驱动

- [ ] 8.1 新增 `src/hal/audio_in.rs`：`AudioInDriver { i2c, i2s_rx }`，`init` 配置 I2C（地址 0x40）写 ES7210 寄存器序列（power up / sample rate 16kHz / unmute），序列参考 Waveshare 示例
- [ ] 8.2 配置 I2S0 RX：BCLK/WS/DIN 引脚来自 BoardProfile，采样率 `OPUS_SAMPLE_RATE`=16000 / 16bit / 单声道 / DMA 缓冲 4 帧×320B
- [ ] 8.3 启动 I2S RX DMA 空跑（不读数据），等待 100ms 验证无 panic / 无 I2S 错误中断

## 9. ES8311 + NS4150B 播放驱动

- [ ] 9.1 新增 `src/hal/audio_out.rs`：`AudioOutDriver { i2c, i2s_tx, pa_ctrl }`，`init` 配置 I2C（地址 0x18）写 ES8311 寄存器序列，配置 I2S0 TX（与 RX 共用 controller 的 TX 通道，引脚 BCLK/WS/DOUT 来自 BoardProfile，采样率 16kHz / 16bit / 单声道 / DMA 缓冲）
- [ ] 9.2 配置 `PA_CTRL`=GPIO15 为 `PinDriver::output` 初始 low
- [ ] 9.3 启动 I2S TX DMA 空跑（输出 zero），PA 保持 low
- [ ] 9.4 实现 `pa_enable(on: bool)`：纯 GPIO 开关——`on=true` 拉高 PA_CTRL、`on=false` 拉低 PA_CTRL，无 delay / 软启动时序（D6：软启动由 change 05 AudioService 负责）

## 10. ESP-NOW 射频驱动

- [ ] 10.1 新增 `src/hal/radio.rs`：`RadioDriver { wifi: EspWifi, espnow: EspNow }`，`init` 先 `EspWifi::new` STA 模式，再 `EspNow::new`，初始信道 = `DISCOVERY_CHANNEL`=1
- [ ] 10.2 不 add_peer / 不注册回调 / 不发送任何包
- [ ] 10.3 暴露 `espnow(&self) -> &EspNow` 供 change 06 调用 add_peer

## 11. Hal 聚合与 main 接入

- [ ] 11.1 在 `src/hal/mod.rs` 注册子模块：`pub mod backlight; pub mod lcd; ...`
- [ ] 11.2 在 `src/main.rs` 临时调用 `let hal = Hal::init(peripherals)?;`（仅在 init 后 `info!` 打印 "Hal init OK"，不接入业务；保留 slint 占位主循环）
- [ ] 11.3 确认 `Hal` 与各 driver 为 `Send`（编译验证：`fn assert_send<T: Send>() {}; assert_send::<Hal>();`）

## 12. 构建验证

- [ ] 12.1 `cargo build` 零错误通过（第三方 crate 警告可接受）
- [ ] 12.2 `cargo build --release` 通过

## 13. 烧录验收

- [ ] 13.1 连接 Waveshare ESP32-C6-Touch-LCD-1.54，`cargo espflash flash --port <port> --release`
- [ ] 13.2 上电后串口监视器（115200 8N1）确认按顺序打印 8 行 init OK：Backlight / Lcd / Touch / Buttons(含 PWR) / Battery / AudioIn / AudioOut / Radio
- [ ] 13.3 LCD 背光点亮，屏幕仍显示 change 01 的 "Intercom Boot OK"（slint 主循环未改）
- [ ] 13.4 触摸屏 / 按键 / ADC 暂无可见业务响应（业务在 change 04+），仅验证 init 不 panic
- [ ] 13.5 用万用表测 PA_CTRL=GPIO15 在 `init` 后为低电平（PA 关闭，避免爆音）

## 14. 收尾

- [ ] 14.1 提交 commit：`feat: HAL/BSP drivers for all peripherals (change 02/17)`，包含 `src/hal/` 全部文件与 `board_profile.rs` 常量追加
- [ ] 14.2 在 commit message 注明后续 change 03-06 将在此 BSP 句柄上实现 Service trait
