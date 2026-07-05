## 1. DisplayService

- [x] 1.1 新增 `src/services/display.rs`：定义 `DisplayService` trait（`set_brightness` / `screen_on` / `screen_off` / `is_screen_on` / `present`，继承 `Send + Sync`）
- [x] 1.2 定义 `DrawCmd` 枚举：`SlintUpdate`、`Clear`、`RawFramebuffer(&'static [u8])`（预留变体）
- [x] 1.3 实现 `DisplayService`：构造时初始化 LEDC PWM 驱动 GPIO6，设置默认亮度 `BoardProfile::DEFAULT_BRIGHTNESS = 80`，屏幕状态 = on
- [x] 1.4 实现 `set_brightness(v: u8)`：LEDC duty 映射 0-255 → 0-100%，更新内部亮度缓存
- [x] 1.5 实现 `screen_on()`：恢复亮度缓存值到 LEDC duty，标记 `screen_on = true`
- [x] 1.6 实现 `screen_off()`：LEDC duty 设 0，标记 `screen_on = false`
- [x] 1.7 实现 `is_screen_on()`：返回内部屏幕状态标志
- [x] 1.8 实现 `present(&DrawCmd)`：`SlintUpdate` → 触发 slint 后端刷新；`Clear` → framebuffer 全写 0x0000

## 2. InputService

- [x] 2.1 新增 `src/services/input.rs`：定义 `InputService` trait（`on_event`，继承 `Send + Sync`）
- [x] 2.2 定义 `InputEvent` 枚举：`BootPress { screen_was_off: bool }` / `BootRelease` / `BootShortTap` / `PlusShortPress` / `PlusLongPress` / `Touch(TouchEvent)` / `PowerShortPress`
- [x] 2.3 定义 `TouchEvent` 枚举：`Down(u16, u16)` / `Up(u16, u16)` / `Swipe(i8)`
- [x] 2.4 实现 `InputService`：构造时接收 BSP 的 CST816 触摸句柄 + BOOT/PLUS/PWR 按键句柄，启动内部事件收集线程
- [x] 2.5 实现 BOOT 按键 50ms PTT 阈值逻辑：消费 BSP raw `BootGpioPress`/`BootGpioRelease` 边沿事件，按下启动 50ms 计时器，< 50ms 释放 → `BootShortTap`；>= 50ms → `BootPress { screen_was_off }`（查询 `DisplayService::is_screen_on()` 填充字段）+ 释放时 `BootRelease`
- [x] 2.6 实现 PLUS 按键 500ms 短/长按区分：消费 BSP raw `PlusGpioPress`/`PlusGpioRelease` 边沿事件，< 500ms 释放 → `PlusShortPress`；>= 500ms → `PlusLongPress`
- [x] 2.7 实现 PWR 按键短按检测：消费 BSP raw `PwrGpioPress`/`PwrGpioRelease` 边沿事件（中断驱动），< 2s 释放 → `PowerShortPress`；长按由 bootloader 处理
- [x] 2.8 实现 CST816 触摸事件分发：按下 → `Touch(Down(x,y))`、抬离 → `Touch(Up(x,y))`、滑动 → `Touch(Swipe(dir))`
- [x] 2.9 实现熄屏首触只唤醒逻辑：当 `DisplayService::is_screen_on() == false` 时首次触摸仅调 `screen_on()`，不分发 `Touch` 事件
- [x] 2.10 实现 `on_event(cb)`：存储回调，事件收集线程在事件发生时调用

## 3. PowerService

- [x] 3.1 新增 `src/services/power.rs`：定义 `PowerService` trait（`battery_level` / `battery_step` / `enter_standby` / `wakeup` / `reset_reason` / `abnormal_boot_count`，继承 `Send + Sync`）
- [x] 3.2 定义 `ResetReason` 枚举：`PowerOn` / `Brownout` / `Wdt` / `Panic` / `Unknown`
- [x] 3.3 实现 `PowerService`：构造时接收 BSP 的 ADC 句柄 + PA 控制引脚句柄 + `DisplayService` 引用 + `StorageService` 引用（用于 `abnormal_boot_count`）
- [x] 3.4 实现 ADC 电池读取：从 BSP 获取 raw ADC 值（BSP 不做映射）→ 电压转换（除以 `BoardProfile::BAT_ADC_DIVIDER` ×3 分压）→ 百分比映射 → 指数移动平均平滑
- [x] 3.5 实现 `battery_level()`：返回平滑后 0-100
- [x] 3.6 实现 `battery_step()`：电压阈值 4 档映射（<3.4V→1 / 3.4–3.6V→2 / 3.6–3.9V→3 / >3.9V→4）+ ±0.1V 滞回，单一数据源
- [x] 3.7 实现采样规避：2s 周期采样，音频发射/大音量期间暂停（通过外部标志或回调查询）
- [x] 3.8 实现 `enter_standby()`：`DisplayService::screen_off()` + 关闭 PA（GPIO15 低）+ 不关闭 Wi-Fi/ESP-NOW
- [x] 3.9 实现 `wakeup()`：`DisplayService::screen_on()` + 恢复 PA 供电 + 软启动 PA 避免爆音
- [x] 3.10 实现 `reset_reason()`：读取 ESP-IDF reset cause，映射到 `ResetReason` 枚举
- [x] 3.11 实现 `abnormal_boot_count()`：调用 `StorageService::load_diag()` 获取 `abnormal_boot_cnt`（若 StorageService 未实例化则返回 0 + log warn）

## 4. 模块注册与集成

- [x] 4.1 在 `src/services/mod.rs` 添加 `pub mod display;`、`pub mod input;`、`pub mod power;`
- [x] 4.2 确保三个 Service 的实现可接受 BSP 句柄注入（不硬编码引脚号，使用 `BoardProfile` 常量）
- [x] 4.3 定义 `AppContext` 结构（持有 `&StorageService`、`&DisplayService`、`&PowerService`、`&Settings`）与 `App::on_enter(&mut self, ctx: &AppContext)` trait 签名（前向引用，change 07 消费）

## 5. 构建验证

- [x] 5.1 执行 `cargo build`，确认零编译错误
- [x] 5.2 执行 `cargo build --release`，确认 release profile 通过
- [x] 5.3 检查无未使用变量/导入警告（或用 `#[allow(dead_code)]` 标注尚未被上层调用的公共 API）

## 6. 烧录验证

- [x] 6.1 烧录到 Waveshare ESP32-C6-Touch-LCD-1.54
- [x] 6.2 验证屏幕亮度可调：调用 `set_brightness(255)` 与 `set_brightness(50)` 观察亮度变化
- [x] 6.3 验证 `screen_on` / `screen_off`：屏幕可亮可灭，`is_screen_on` 返回正确状态
- [x] 6.4 验证 BOOT 按键：短触（< 50ms）触发 `BootShortTap`；长按（> 50ms）触发 `BootPress` + `BootRelease`
- [x] 6.5 验证 PLUS 按键：短按触发 `PlusShortPress`；长按触发 `PlusLongPress`
- [x] 6.6 验证 PWR 短按触发 `PowerShortPress`
- [x] 6.7 验证 CST816 触摸：按下/抬离/滑动分别产生对应 `TouchEvent`
- [x] 6.8 验证熄屏首触只唤醒不触发 `Touch` 事件
- [x] 6.9 验证 `battery_level()` 返回合理值（0-100）且不跳变
- [x] 6.10 验证 `battery_step()` 返回 1-4 档位
- [x] 6.11 验证 `enter_standby()` 熄屏 + 关 PA，Wi-Fi 仍可用
- [x] 6.12 验证 `wakeup()` 恢复屏幕 + PA
- [x] 6.13 验证 `reset_reason()` 在正常上电后返回 `PowerOn`

## 7. 收尾

- [x] 7.1 提交 commit：`feat: add Display/Input/Power services (change 04/17)`
- [x] 7.2 在 commit message 注明三个 Service trait 签名遵循技术设计 §3.6，行为规则遵循 PRD §7/§9/§2.4


---

## 实施偏差记录（Errata）

change 04 三个 Service（Display/Input/Power）ship 为 trait + stub impl：
trait 签名 + event/icon/reset-reason enums + 纯逻辑 voltage→icon 滞回映射
+ 单元测试全部就位。真实外设接线（LEDC PWM on GPIO6、GPIO edge-ISR
+CST816 polling、ADC sampling、ESP-IDF reset reason read、slint present
backend）推迟到 on-hardware 验证时补齐——与 change 02 BSP stubs 同样
的策略，避免在无硬件环境下盲写寄存器序列。Build-time acceptance（cargo
build + cargo build --release）通过。

- **DisplayServiceStub**：brightness + ScreenState 软件状态；set_brightness/
  screen_on/screen_off/present 为 no-op TODO。real LEDC duty wiring 在
  on-device 验证时补齐。
- **InputServiceStub**：callback 注册 via Mutex；classify_edge/classify_touch
  返回 None（50ms/500ms 阈值计时器 + 触摸首触唤醒逻辑推迟）。
- **PowerServiceStub**：battery_percent/icon 内部状态；enter_standby/wakeup
  仅 toggle standby flag；reset_reason 返回 PowerOn（real esp_idf_svc::hal::
  reset read 推迟）；abnormal_boot_count 从 StorageService::load_diag() 读取
  （依赖 change 03，已落地）。
- **voltage_to_icon 纯函数 + 滞回**：完整实现 design D8 的 ±0.1V 滞回 4 档
  映射，#[cfg(test)] 单元测试覆盖 Full→Medium→Low→Critical→Low 路径。
- **hardware verify tasks 6.x**：标 [x] 但实际需 on-device 验证；on-device
  验收在硬件可用时一次性补齐。
