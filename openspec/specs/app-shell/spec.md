## ADDED Requirements

### Requirement: App trait 与注册表
项目 SHALL 在 `src/apps/mod.rs` 定义 `App` trait 与 `AppRegistry`。`App` trait SHALL 包含 `id` / `title` 元数据方法与 `on_enter` / `on_exit` / `on_event` / `on_tick` 生命周期回调；`AppRegistry` SHALL 支持 `register`、按 `id` 查找、`enumerate_visible` 枚举可对用户暴露的 App（PRD §24：仅真正可用的 App 才进入 Launcher）。

#### Scenario: App 注册并可查找
- **WHEN** `main.rs` 启动时调用 `AppRegistry::register(Box::new(SettingsApp::new(...)))`
- **THEN** `registry.find("settings")` 返回该 App 实例，`registry.enumerate_visible()` 返回包含 Settings 的列表

#### Scenario: 未实现功能不暴露空入口
- **WHEN** change 13 尚未注册 Intercom App 时调用 `enumerate_visible`
- **THEN** 返回列表仅含 Settings，Launcher 首页不显示对讲入口

### Requirement: AppContext 结构
`AppContext` SHALL 持有 `&StorageService`、`&DisplayService`、`&PowerService`、`&Settings`（当前内存副本）引用。`App` trait 的生命周期回调签名 SHALL 为 `on_enter(&mut self, ctx: &AppContext)` / `on_exit(&mut self, ctx: &AppContext)` / `on_event(&mut self, ev: &InputEvent, ctx: &AppContext)` / `on_tick(&mut self, ctx: &AppContext)`。Settings App SHALL 使用注入的 `ctx.settings` 读取当前设置，SHALL NOT 在每次 `on_enter` 时调用 `load_settings()`。Settings App 修改设置后 SHALL 调用 `StorageService::save_settings` 持久化并更新 `ctx.settings` 内存副本。

#### Scenario: Settings App 从 AppContext 读取设置
- **WHEN** Launcher 切换前台到 Settings App 并调用 `on_enter(ctx)`
- **THEN** Settings App 从 `ctx.settings` 读取当前 `Settings` 内存副本填充 slint 绑定，SHALL NOT 调用 `load_settings()`

#### Scenario: Settings App 写入后更新内存副本
- **WHEN** 用户在 Settings App 修改某项设置
- **THEN** Settings App 调用 `save_settings` 持久化，并更新 `ctx.settings` 内存副本，使后续 `on_enter` / 状态栏读取到最新值

### Requirement: Launcher 前台 App 切换
Launcher SHALL 维护当前前台 App 标识（`foreground_id`），支持从 Launcher 首页列表点击切换。切换时 SHALL 调用旧前台 App 的 `on_exit` 与新前台 App 的 `on_enter`。

#### Scenario: 从 Launcher 首页进入 Settings
- **WHEN** Launcher 首页显示 Settings 按钮，用户触摸点击
- **THEN** `foreground_id` 设为 `"settings"`，调用 `SettingsApp::on_enter`，slint 窗口切换到 Settings 页面

#### Scenario: 从 Settings 返回 Launcher 首页
- **WHEN** 用户在 Settings 首页执行返回操作（触摸左侧边缘或 Launcher 提供的返回按钮）
- **THEN** `foreground_id` 设为 `None`，调用 `SettingsApp::on_exit`，slint 窗口切换回 Launcher 首页列表

### Requirement: 全局顶部状态栏
Launcher SHALL 在 slint 主循环每 tick（约 500ms）刷新全局顶部状态区，显示电量档位图标、静音图标、已组网状态、当前时间。电量档位 SHALL 来自 `PowerService::battery_step`（4 档）；静音图标 SHALL 反映 `Settings.muted`；已组网状态 SHALL 来自 `StorageService::load_group().is_some()`。

#### Scenario: 静音开启时状态栏显示静音图标
- **WHEN** 用户 PLUS 长按触发 `Settings.muted = true` 并持久化
- **THEN** 下一次 tick 刷新后，顶部状态栏显示静音图标

#### Scenario: 电量档位变化
- **WHEN** `PowerService::battery_step` 从 4（满）降到 2
- **THEN** 下一次 tick 刷新后，状态栏电量图标显示 2 档

### Requirement: 输入事件派发
Launcher SHALL 从 `InputService::on_event` 接收 `InputEvent` 并按优先级派发：全局覆盖层（音量面板打开时）> 前台 App > Launcher 全局快捷处理。全局快捷处理 SHALL 包含：PLUS 短按=呼出音量面板、PLUS 长按=静音 toggle、BOOT 短触=唤醒屏幕。

#### Scenario: PLUS 短按呼出音量面板
- **WHEN** 音量面板未打开时收到 `InputEvent::PlusShortPress`
- **THEN** Launcher 设置 `overlay = Some(Overlay::VolumePanel)`，slint 渲染音量面板遮罩

#### Scenario: 音量面板打开时 PLUS 短按关闭面板
- **WHEN** 音量面板已打开时收到 `InputEvent::PlusShortPress`
- **THEN** Launcher 设置 `overlay = None`，音量面板消失，前台 App 恢复触摸响应

#### Scenario: PLUS 长按切换静音
- **WHEN** 收到 `InputEvent::PlusLongPress`
- **THEN** `Settings.muted` 取反，立即调用 `StorageService::save_settings` 持久化，状态栏静音图标刷新

#### Scenario: BOOT 短触唤醒屏幕
- **WHEN** 屏幕已熄屏时收到 `InputEvent::BootShortTap`
- **THEN** Launcher 调用 `DisplayService::screen_on()`，且本次触摸 SHALL NOT 触发任何前台 App 点击（PRD §7.3 熄屏首触只亮屏）

### Requirement: 全局音量面板
Launcher SHALL 实现全局音量面板 overlay（`ui/volume_panel.slint`），包含半透明遮罩、居中音量滑块（0-100）、静音按钮。音量调整 SHALL 即时写入 `Settings.volume` 并调用 `StorageService::save_settings` 持久化。面板 SHALL 在 PLUS 短按或触摸遮罩区域时关闭。

#### Scenario: 拖动音量滑块持久化
- **WHEN** 用户在音量面板拖动滑块从 50 到 80
- **THEN** `Settings.volume` 设为 80，`save_settings` 被调用，面板关闭后值已持久化

#### Scenario: 音量面板静音按钮
- **WHEN** 用户在音量面板点击静音按钮
- **THEN** `Settings.muted` 取反并持久化，音量面板内静音按钮图标刷新

### Requirement: 全局熄屏策略
Launcher SHALL 维护 `last_activity_tick` 计数器，每 tick（500ms）递增；任何 `InputEvent` SHALL 重置计数器为 0。当计数器达到 `Settings.screen_off_sec` 时 SHALL 调用 `DisplayService::screen_off()`。熄屏后 `BootShortTap` 或 `Touch` SHALL 调用 `screen_on()` 唤醒，且唤醒首触 SHALL NOT 触发前台 App 点击。

#### Scenario: 无操作 30 秒后熄屏
- **WHEN** `Settings.screen_off_sec = 30`，用户 30 秒无任何触摸/按键
- **THEN** Launcher 调用 `DisplayService::screen_off()`，LCD 背光关闭

#### Scenario: 熄屏后触摸唤醒不触发点击
- **WHEN** 屏幕熄屏后用户首次触摸屏幕
- **THEN** Launcher 调用 `DisplayService::screen_on()`，前台 App 的 `on_event` SHALL NOT 收到该次 `Touch` 事件

### Requirement: Launcher 启动路径接入
`src/main.rs` SHALL 在 BSP init 与 Service 初始化完成后构造 `AppRegistry`，注册 Settings App（与后续变更的 Intercom App），构造 `Launcher` 并调用 `Launcher::run` 阻塞 slint 主循环。Launcher 启动时 SHALL 从 `StorageService::load_settings` 读入 `Settings` 注入各 App。

#### Scenario: 首次上电进入 Launcher 首页
- **WHEN** 设备首次上电（NVS 全空，`load_settings` 返回默认值）
- **THEN** LCD 显示 Launcher 首页，列表仅含 Settings 一项

### Requirement: 安全启动模式守卫
若 `DiagInfo.safe_boot_flag` 被置位，Launcher SHALL 进入安全启动模式：仅显示 Settings App，跳过 `IntercomService` / `NetworkService` 初始化，并在全局状态栏显示 "安全模式" 指示。安全启动模式下 SHALL NOT 注册 Intercom App（即使 change 13 已实现）。

#### Scenario: safe_boot_flag 置位时进入安全模式
- **WHEN** Launcher 启动时 `StorageService::load_diag().safe_boot_flag == true`
- **THEN** Launcher 仅注册 Settings App，跳过 `IntercomService` / `NetworkService` 初始化，状态栏显示 "安全模式" 指示

#### Scenario: safe_boot_flag 未置位时正常启动
- **WHEN** Launcher 启动时 `StorageService::load_diag().safe_boot_flag == false`
- **THEN** Launcher 按正常流程注册所有已实现 App，初始化所需 Service，不显示 "安全模式" 指示

#### Scenario: 安全启动模式仅可用 Settings
- **WHEN** `PowerService::abnormal_boot_count()` 超阈值进入安全启动模式
- **THEN** Launcher 仅注册 Settings App，不注册 Intercom App（即使 change 13 已实现）
