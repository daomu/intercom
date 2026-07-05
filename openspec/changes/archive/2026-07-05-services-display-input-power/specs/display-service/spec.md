## ADDED Requirements

### Requirement: DisplayService trait 定义
项目 SHALL 在 `src/services/display.rs` 中定义 `DisplayService` trait，包含以下方法签名：`fn set_brightness(&self, v: u8)`、`fn screen_on(&self)`、`fn screen_off(&self)`、`fn is_screen_on(&self) -> bool`、`fn present(&self, view: &DrawCmd)`。该 trait SHALL 继承 `Send + Sync`。

#### Scenario: trait 可被其他模块引用
- **WHEN** 在 `src/apps/` 或 `src/intercom/` 中 `use crate::services::display::DisplayService`
- **THEN** trait 及其方法签名在编译期可见，可构造 `Box<dyn DisplayService>` 传给上层

### Requirement: 背光 PWM 调光
`DisplayService::set_brightness(v: u8)` SHALL 通过 ESP-IDF LEDC 驱动在 GPIO6 输出 PWM，duty cycle 映射 v 值 0-255 到 0-100% 占空比。`v = 0` 等效于关闭背光。

#### Scenario: 设置亮度
- **WHEN** 调用 `set_brightness(128)`
- **THEN** GPIO6 背光 PWM duty 约为 50%，屏幕可见中等亮度

#### Scenario: 设置零亮度
- **WHEN** 调用 `set_brightness(0)`
- **THEN** 背光 PWM duty 为 0，屏幕视觉熄灭

### Requirement: 亮屏/熄屏状态管理
`screen_on()` SHALL 恢复背光至当前保存的亮度值（默认 `BoardProfile::DEFAULT_BRIGHTNESS = 80`）并标记屏幕为亮。`screen_off()` SHALL 将背光 duty 设为 0 并标记屏幕为灭。`is_screen_on()` SHALL 返回当前屏幕开关状态。

#### Scenario: 熄屏后查询状态
- **WHEN** 调用 `screen_off()` 后调用 `is_screen_on()`
- **THEN** 返回 `false`

#### Scenario: 亮屏后查询状态
- **WHEN** 调用 `screen_on()` 后调用 `is_screen_on()`
- **THEN** 返回 `true`

#### Scenario: 熄屏后亮屏恢复亮度
- **WHEN** 先 `set_brightness(200)` 再 `screen_off()` 再 `screen_on()`
- **THEN** 亮屏后亮度恢复为 200，而非默认值

### Requirement: 绘制指令提交
`present(&self, view: &DrawCmd)` SHALL 接受 `DrawCmd` 枚举并执行渲染提交。`DrawCmd::SlintUpdate` SHALL 触发 slint 后端将当前窗口内容刷新到 ST7789 framebuffer。`DrawCmd::Clear` SHALL 将屏幕清为全黑。

#### Scenario: 提交 slint 刷新
- **WHEN** 调用 `present(&DrawCmd::SlintUpdate)`
- **THEN** slint 后端执行一次 framebuffer 刷新，LCD 显示更新后的画面

#### Scenario: 清屏
- **WHEN** 调用 `present(&DrawCmd::Clear)`
- **THEN** LCD framebuffer 全部写入 0x0000（黑色），屏幕显示全黑

### Requirement: DrawCmd 枚举定义
项目 SHALL 定义 `DrawCmd` 枚举，包含至少 `SlintUpdate` 和 `Clear` 两个变体。MAY 包含 `RawFramebuffer(&'static [u8])` 变体作为预留。

#### Scenario: 枚举可构造与匹配
- **WHEN** 构造 `DrawCmd::SlintUpdate` 并在 `present` 实现中 `match` 分发
- **THEN** 编译通过，无未覆盖分支警告

### Requirement: 默认亮度与初始化
`DisplayService` 实现 SHALL 在构造时将亮度设为 `BoardProfile::DEFAULT_BRIGHTNESS`（80），屏幕状态为亮（`is_screen_on() == true`）。

#### Scenario: 首次构造后状态
- **WHEN** 实例化 `DisplayService` 实现
- **THEN** `is_screen_on()` 返回 `true`，背光亮度为 80

### Requirement: 模块注册
`src/services/mod.rs` SHALL 包含 `pub mod display;` 以注册 display 子模块。

#### Scenario: 模块可见
- **WHEN** 在 `src/main.rs` 或其他模块中 `use crate::services::display::DisplayService`
- **THEN** 路径解析成功，编译通过

### Requirement: AppContext 结构（前向引用，change 07 消费）
项目 SHALL 定义 `AppContext` 结构，持有 `&StorageService`、`&DisplayService`、`&PowerService`、`&Settings`（当前内存中的副本）。`App` trait SHALL 定义 `fn on_enter(&mut self, ctx: &AppContext)`。此结构在本变更中定义（因三个 Service 均在本变更首次定义），但实际由 change 07（app-shell-settings）消费——Settings App 使用注入的 `ctx.settings` 而非每次 `on_enter` 调用 `load_settings()`。本变更仅提供结构与 trait 签名，不实现具体 App。

> **交叉引用**：change 07 将实现 `App::on_enter` 的具体逻辑，使用 `AppContext` 注入的服务引用。若 `AppContext` 更自然地归属 change 07，本变更保留定义权但标注为前向引用。

#### Scenario: AppContext 可构造
- **WHEN** 在 change 07 中构造 `AppContext { storage, display, power, settings }`
- **THEN** 编译通过，所有字段类型为本变更定义的 Service trait 引用

#### Scenario: App::on_enter 接收注入上下文
- **WHEN** Launcher 切换到某 App 时调用 `app.on_enter(&ctx)`
- **THEN** App 通过 `ctx.display` / `ctx.power` / `ctx.settings` 访问服务，无需自行构造
