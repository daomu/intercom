## 1. DisplayService

- [ ] 1.1 新增 `src/services/display.rs`：定义 `DisplayService` trait（`set_brightness` / `screen_on` / `screen_off` / `is_screen_on` / `present`，继承 `Send + Sync`）
- [ ] 1.2 定义 `DrawCmd` 枚举：`SlintUpdate`、`Clear`、`RawFramebuffer(&'static [u8])`（预留变体）
- [ ] 1.3 实现 `DisplayService`：构造时初始化 LEDC PWM 驱动 GPIO6，设置默认亮度 `BoardProfile::DEFAULT_BRIGHTNESS = 80`，屏幕状态 = on
- [ ] 1.4 实现 `set_brightness(v: u8)`：LEDC duty 映射 0-255 → 0-100%，更新内部亮度缓存
- [ ] 1.5 实现 `screen_on()`：恢复亮度缓存值到 LEDC duty，标记 `screen_on = true`
- [ ] 1.6 实现 `screen_off()`：LEDC duty 设 0，标记 `screen_on = false`
- [ ] 1.7 实现 `is_screen_on()`：返回内部屏幕状态标志
- [ ] 1.8 实现 `present(&DrawCmd)`：`SlintUpdate` → 触发 slint 后端刷新；`Clear` → framebuffer 全写 0x0000

## 2. InputService

- [ ] 2.1 新增 `src/services/input.rs`：定义 `InputService` trait（`on_event`，继承 `Send + Sync`）
- [ ] 2.2 定义 `InputEvent` 枚举：`BootPress { screen_was_off: bool }` / `BootRelease` / `BootShortTap` / `PlusShortPress` / `PlusLongPress` / `Touch(TouchEvent)` / `PowerShortPress`
- [ ] 2.3 定义 `TouchEvent` 枚举：`Down(u16, u16)` / `Up(u16, u16)` / `Swipe(i8)`
- [ ] 2.4 实现 `InputService`：构造时接收 BSP 的 CST816 触摸句柄 + BOOT/PLUS/PWR 按键句柄，启动内部事件收集线程
- [ ] 2.5 实现 BOOT 按键 50ms PTT 阈值逻辑：消费 BSP raw `BootGpioPress`/`BootGpioRelease` 边沿事件，按下启动 50ms 计时器，< 50ms 释放 → `BootShortTap`；>= 50ms → `BootPress { screen_was_off }`（查询 `DisplayService::is_screen_on()` 填充字段）+ 释放时 `BootRelease`
- [ ] 2.6 实现 PLUS 按键 500ms 短/长按区分：消费 BSP raw `PlusGpioPress`/`PlusGpioRelease` 边沿事件，< 500ms 释放 → `PlusShortPress`；>= 500ms → `PlusLongPress`
- [ ] 2.7 实现 PWR 按键短按检测：消费 BSP raw `PwrGpioPress`/`PwrGpioRelease` 边沿事件（中断驱动），< 2s 释放 → `PowerShortPress`；长按由 bootloader 处理
- [ ] 2.8 实现 CST816 触摸事件分发：按下 → `Touch(Down(x,y))`、抬离 → `Touch(Up(x,y))`、滑动 → `Touch(Swipe(dir))`
- [ ] 2.9 实现熄屏首触只唤醒逻辑：当 `DisplayService::is_screen_on() == false` 时首次触摸仅调 `screen_on()`，不分发 `Touch` 事件
- [ ] 2.10 实现 `on_event(cb)`：存储回调，事件收集线程在事件发生时调用

## 3. PowerService

- [ ] 3.1 新增 `src/services/power.rs`：定义 `PowerService` trait（`battery_level` / `battery_step` / `enter_standby` / `wakeup` / `reset_reason` / `abnormal_boot_count`，继承 `Send + Sync`）
- [ ] 3.2 定义 `ResetReason` 枚举：`PowerOn` / `Brownout` / `Wdt` / `Panic` / `Unknown`
- [ ] 3.3 实现 `PowerService`：构造时接收 BSP 的 ADC 句柄 + PA 控制引脚句柄 + `DisplayService` 引用 + `StorageService` 引用（用于 `abnormal_boot_count`）
- [ ] 3.4 实现 ADC 电池读取：从 BSP 获取 raw ADC 值（BSP 不做映射）→ 电压转换（除以 `BoardProfile::BAT_ADC_DIVIDER` ×3 分压）→ 百分比映射 → 指数移动平均平滑
- [ ] 3.5 实现 `battery_level()`：返回平滑后 0-100
- [ ] 3.6 实现 `battery_step()`：电压阈值 4 档映射（<3.4V→1 / 3.4–3.6V→2 / 3.6–3.9V→3 / >3.9V→4）+ ±0.1V 滞回，单一数据源
- [ ] 3.7 实现采样规避：2s 周期采样，音频发射/大音量期间暂停（通过外部标志或回调查询）
- [ ] 3.8 实现 `enter_standby()`：`DisplayService::screen_off()` + 关闭 PA（GPIO15 低）+ 不关闭 Wi-Fi/ESP-NOW
- [ ] 3.9 实现 `wakeup()`：`DisplayService::screen_on()` + 恢复 PA 供电 + 软启动 PA 避免爆音
- [ ] 3.10 实现 `reset_reason()`：读取 ESP-IDF reset cause，映射到 `ResetReason` 枚举
- [ ] 3.11 实现 `abnormal_boot_count()`：调用 `StorageService::load_diag()` 获取 `abnormal_boot_cnt`（若 StorageService 未实例化则返回 0 + log warn）

## 4. 模块注册与集成

- [ ] 4.1 在 `src/services/mod.rs` 添加 `pub mod display;`、`pub mod input;`、`pub mod power;`
- [ ] 4.2 确保三个 Service 的实现可接受 BSP 句柄注入（不硬编码引脚号，使用 `BoardProfile` 常量）
- [ ] 4.3 定义 `AppContext` 结构（持有 `&StorageService`、`&DisplayService`、`&PowerService`、`&Settings`）与 `App::on_enter(&mut self, ctx: &AppContext)` trait 签名（前向引用，change 07 消费）

## 5. 构建验证

- [ ] 5.1 执行 `cargo build`，确认零编译错误
- [ ] 5.2 执行 `cargo build --release`，确认 release profile 通过
- [ ] 5.3 检查无未使用变量/导入警告（或用 `#[allow(dead_code)]` 标注尚未被上层调用的公共 API）

## 6. 烧录验证

- [ ] 6.1 烧录到 Waveshare ESP32-C6-Touch-LCD-1.54
- [ ] 6.2 验证屏幕亮度可调：调用 `set_brightness(255)` 与 `set_brightness(50)` 观察亮度变化
- [ ] 6.3 验证 `screen_on` / `screen_off`：屏幕可亮可灭，`is_screen_on` 返回正确状态
- [ ] 6.4 验证 BOOT 按键：短触（< 50ms）触发 `BootShortTap`；长按（> 50ms）触发 `BootPress` + `BootRelease`
- [ ] 6.5 验证 PLUS 按键：短按触发 `PlusShortPress`；长按触发 `PlusLongPress`
- [ ] 6.6 验证 PWR 短按触发 `PowerShortPress`
- [ ] 6.7 验证 CST816 触摸：按下/抬离/滑动分别产生对应 `TouchEvent`
- [ ] 6.8 验证熄屏首触只唤醒不触发 `Touch` 事件
- [ ] 6.9 验证 `battery_level()` 返回合理值（0-100）且不跳变
- [ ] 6.10 验证 `battery_step()` 返回 1-4 档位
- [ ] 6.11 验证 `enter_standby()` 熄屏 + 关 PA，Wi-Fi 仍可用
- [ ] 6.12 验证 `wakeup()` 恢复屏幕 + PA
- [ ] 6.13 验证 `reset_reason()` 在正常上电后返回 `PowerOn`

## 7. 收尾

- [ ] 7.1 提交 commit：`feat: add Display/Input/Power services (change 04/17)`
- [ ] 7.2 在 commit message 注明三个 Service trait 签名遵循技术设计 §3.6，行为规则遵循 PRD §7/§9/§2.4
