## ADDED Requirements

### Requirement: InputService trait 定义
项目 SHALL 在 `src/services/input.rs` 中定义 `InputService` trait，包含方法 `fn on_event(&self, cb: Box<dyn Fn(InputEvent) + Send + Sync>)`。该 trait SHALL 继承 `Send + Sync`。

#### Scenario: 回调注册
- **WHEN** 调用 `on_event(cb)` 传入一个闭包
- **THEN** 后续输入事件触发时该闭包被调用，收到对应的 `InputEvent`

### Requirement: InputEvent 枚举定义
项目 SHALL 定义 `InputEvent` 枚举，包含以下变体：`BootPress { screen_was_off: bool }`、`BootRelease`、`BootShortTap`、`PlusShortPress`、`PlusLongPress`、`Touch(TouchEvent)`、`PowerShortPress`。`BootPress` 携带 `screen_was_off: bool` 字段，指示按下 BOOT 时屏幕是否处于熄灭状态——上层（change 10 voice-ptt）据此决定是否在熄屏状态下直接进入 PTT 而不唤醒屏幕。

#### Scenario: 枚举可匹配
- **WHEN** 在回调闭包中 `match event { InputEvent::BootPress { screen_was_off } => ..., _ => ... }`
- **THEN** 编译通过，所有变体可见，`screen_was_off` 字段可读取

#### Scenario: 熄屏 PTT 携带 screen_was_off=true
- **WHEN** 屏幕熄灭时用户长按 BOOT 超过 50ms
- **THEN** 回调收到 `InputEvent::BootPress { screen_was_off: true }`

#### Scenario: 亮屏 PTT 携带 screen_was_off=false
- **WHEN** 屏幕已亮时用户长按 BOOT 超过 50ms
- **THEN** 回调收到 `InputEvent::BootPress { screen_was_off: false }`

### Requirement: TouchEvent 枚举定义
项目 SHALL 定义 `TouchEvent` 枚举，包含以下变体：`Down(u16, u16)`（x, y 坐标）、`Up(u16, u16)`、`Swipe(i8)`（滑动方向，正=右、负=左）。

#### Scenario: 触摸坐标传递
- **WHEN** CST816 报告触摸点 (120, 100)
- **THEN** 回调收到 `InputEvent::Touch(TouchEvent::Down(120, 100))`

### Requirement: 输入事件分类管线（BSP raw → InputService 分类）
`InputService` SHALL 拥有全部按键阈值分类逻辑，BSP（change 02）仅提供 raw GPIO 边沿事件，不进行去抖/短长按分类。分类管线如下：
- **BOOT（GPIO9）**：BSP 发出 `BootGpioPress` / `BootGpioRelease` raw 边沿事件 → InputService 启动 50ms 计时器：hold >= 50ms 后释放 → `BootPress`（PTT）；释放 < 50ms → `BootShortTap`（唤醒）
- **PLUS（GPIO18）**：BSP 发出 `PlusGpioPress` / `PlusGpioRelease` raw 边沿事件 → InputService 启动 500ms 计时器：< 500ms → `PlusShortPress`；>= 500ms → `PlusLongPress`
- **PWR**：BSP 发出 `PwrGpioPress` / `PwrGpioRelease` raw 边沿事件（中断驱动）→ InputService 启动 2s 计时器：< 2s → `PowerShortPress`；长按由 ESP-IDF bootloader 处理硬件关机，不经软件

#### Scenario: BSP 仅提供 raw 边沿
- **WHEN** BOOT 按键 GPIO9 产生下降沿
- **THEN** BSP 向事件队列推送 `BootGpioPress`，不进行任何去抖或阈值判断

#### Scenario: InputService 消费 raw 事件并分类
- **WHEN** InputService 事件收集线程从队列取出 `BootGpioPress`，50ms 后未收到 `BootGpioRelease`
- **THEN** InputService 发出 `BootPress` 事件（携带当前 `screen_was_off` 状态）

### Requirement: BOOT 按键 PTT 阈值
BOOT 按键按下后 SHALL 启动 50ms 计时。若按下持续时间 < 50ms（在 50ms 内释放），SHALL 发出 `BootShortTap` 事件。若按下持续时间 >= 50ms，SHALL 在 50ms 阈值达成时发出 `BootPress` 事件，并在后续释放时发出 `BootRelease` 事件。

#### Scenario: 短触唤醒
- **WHEN** 用户按下 BOOT 并在 30ms 后释放
- **THEN** 回调收到 `InputEvent::BootShortTap`，不收到 `BootPress` 或 `BootRelease`

#### Scenario: 正常 PTT 按压
- **WHEN** 用户按下 BOOT 并保持 200ms 后释放
- **THEN** 回调依次收到 `InputEvent::BootPress`（按下后约 50ms 时）和 `InputEvent::BootRelease`（释放时）

#### Scenario: 恰好 50ms 边界
- **WHEN** 用户按下 BOOT 恰好保持 50ms 后释放
- **THEN** 回调收到 `BootPress`（50ms 阈值达成时），随后收到 `BootRelease`（释放时）

### Requirement: PLUS 按键短/长按区分
PLUS 按键按下后 SHALL 启动 500ms 计时。若按下持续时间 < 500ms，SHALL 发出 `PlusShortPress` 事件。若按下持续时间 >= 500ms，SHALL 在 500ms 阈值达成时发出 `PlusLongPress` 事件，释放时不额外发事件。

#### Scenario: PLUS 短按
- **WHEN** 用户按下 PLUS 并在 200ms 后释放
- **THEN** 回调收到 `InputEvent::PlusShortPress`

#### Scenario: PLUS 长按
- **WHEN** 用户按下 PLUS 并保持 800ms 后释放
- **THEN** 回调在约 500ms 时收到 `InputEvent::PlusLongPress`，释放时无额外事件

### Requirement: PWR 按键短按事件
PWR 按键短按（< 2s 释放）SHALL 发出 `PowerShortPress` 事件。PWR 长按（>= 2s）由 ESP-IDF bootloader 处理硬件关机，SHALL NOT 产生软件事件。

#### Scenario: PWR 短按
- **WHEN** 用户短按 PWR 键（按下 500ms 后释放）
- **THEN** 回调收到 `InputEvent::PowerShortPress`

#### Scenario: PWR 长按不产生软件事件
- **WHEN** 用户长按 PWR 键超过 2s
- **THEN** 设备由 bootloader 执行硬件关机，`InputService` 不产生任何 `InputEvent`

### Requirement: 熄屏首触只唤醒
当屏幕处于熄灭状态（`DisplayService::is_screen_on() == false`）时，首次触摸事件 SHALL 仅触发屏幕唤醒（`screen_on`），SHALL NOT 向回调分发 `Touch(TouchEvent)` 事件。亮屏后的后续触摸事件 SHALL 正常分发。

#### Scenario: 熄屏首触唤醒
- **WHEN** 屏幕熄灭时用户触摸 CST816
- **THEN** 屏幕亮起，回调不收到 `Touch` 事件

#### Scenario: 亮屏后触摸正常
- **WHEN** 屏幕已亮，用户触摸 CST816
- **THEN** 回调收到 `InputEvent::Touch(TouchEvent::Down(x, y))`

### Requirement: CST816 触摸事件分发
`InputService` 实现 SHALL 从 CST816 驱动读取触摸数据并分发为 `TouchEvent`：按下时分发 `Down(x, y)`，抬离时分发 `Up(x, y)`，滑动时分发 `Swipe(direction)`。

#### Scenario: 触摸按下
- **WHEN** CST816 报告触摸按下事件，坐标 (80, 60)
- **THEN** 回调收到 `InputEvent::Touch(TouchEvent::Down(80, 60))`

#### Scenario: 触摸抬离
- **WHEN** CST816 报告触摸抬离事件，坐标 (80, 60)
- **THEN** 回调收到 `InputEvent::Touch(TouchEvent::Up(80, 60))`

#### Scenario: 滑动检测
- **WHEN** CST816 报告连续移动且位移超过阈值
- **THEN** 回调收到 `InputEvent::Touch(TouchEvent::Swipe(direction))`，direction 为 +1（右滑）或 -1（左滑）

### Requirement: 模块注册
`src/services/mod.rs` SHALL 包含 `pub mod input;` 以注册 input 子模块。

#### Scenario: 模块可见
- **WHEN** 在 `src/main.rs` 或其他模块中 `use crate::services::input::InputService`
- **THEN** 路径解析成功，编译通过
