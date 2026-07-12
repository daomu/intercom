## ADDED Requirements

### Requirement: App trait 命令式渲染扩展
`App` trait SHALL 新增 `render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx)` 方法与 `hit_test(&self, x: i32, y: i32, ctx: &RenderCtx) -> Option<HitTarget>` 方法。`render` SHALL 读取 app 自身 model 状态 + `ctx` 快照，用 `embedded-graphics` 原语绘制到 framebuffer。`hit_test` SHALL 根据当前布局状态返回命中目标。此扩展替代 slint 声明式重绘——app 主动承担绘制责任。

#### Scenario: app render 绘制到 framebuffer
- **WHEN** 主循环调用 `app.render(&mut fb, &ctx)`
- **THEN** app 当前状态（页面/peer 列表/设置值）被绘制到 fb，fb 随后被 `LcdDriver::present` 推送到 ST7789

#### Scenario: hit_test 返回命中目标
- **WHEN** 触摸 Down 事件坐标 `(x, y)` 命中 Launcher 的 Settings 入口
- **THEN** `LauncherView::hit_test` 返回 `Some(HitTarget::SettingsTile)`，controller 转成 `launcher.launch(AppId::Settings)`

### Requirement: RenderCtx 快照
主循环 SHALL 每 tick（500ms）从 `PowerService` / `RadioDriver` / `StorageService` / `Settings` / RTC 快照当前值到 `RenderCtx` 值类型。`RenderCtx` SHALL 包含：`battery_step` / `signal_bars` / `time_hms` / `is_grouped` / `muted` / `mode` / `fw_version` / `settings` / `safe_mode`。`render` 调用 SHALL 只读快照，SHALL NOT 在渲染时 lock service `Mutex`。

#### Scenario: 状态栏读取电量快照
- **WHEN** 主循环快照 `battery_step = 2` 并调用 `StatusBar::draw(fb, ctx)`
- **THEN** 状态栏电量图标显示 2 档，渲染过程不持有 `PowerService` 锁

### Requirement: UiEvent 队列（替代 invoke_from_event_loop）
项目 SHALL 定义 `UiEvent` 枚举与 `UiEventQueue`（`Arc<Mutex<ArrayDeque<UiEvent, 64>>` 或 `embassy_sync::channel::Channel`），承载跨线程事件投递。`UiEvent` SHALL 至少包含 `Intercom(IntercomEvent)` / `Network(NetworkEvent)` / `Audio(AudioEvent)` / `Dirty` 变体。生产者（ESP-NOW 回调 / 音频线程）SHALL `push_back`，主循环 SHALL 每 tick `pop_front` drain 后派发到 model，并置 dirty 触发重绘。Phase 1 SHALL 一次定义全部 4 个变体；若 `IntercomEvent` / `NetworkEvent` / `AudioEvent` 类型尚未由对应 change 交付，SHALL 使用 `()` 占位类型并标注 TODO，在该 change 交付后替换为真实类型，SHALL NOT 推迟到 Phase 4 才补齐变体定义。占位变体被主循环 drain 时 SHALL 记日志并忽略，SHALL NOT panic。

#### Scenario: espnow 回调投递 IntercomEvent
- **WHEN** ESP-NOW 接收线程收到 `PeerOnline(id, rssi_4)` 事件
- **THEN** `UiEventQueue.push_back(UiEvent::Intercom(PeerOnline))` 被调用，主循环下一 tick drain 后更新 IntercomApp peer 卡片并重绘

#### Scenario: 队列满时丢弃
- **WHEN** UiEvent 队列已达 64 容量且生产者继续 push
- **THEN** 新事件被丢弃并记日志 warn，SHALL NOT 阻塞生产者线程

#### Scenario: Phase 1 占位变体可构造且不 panic
- **WHEN** Phase 1 定义 `UiEvent::Intercom(())` 占位变体（`IntercomEvent` 类型尚未交付）
- **THEN** 代码编译通过，主循环 drain 时 match 该变体 SHALL 记日志并忽略，SHALL NOT panic；等对应 change 交付后替换为真实 `IntercomEvent` 类型

### Requirement: 主渲染循环
`src/main.rs` SHALL 移除 idle 循环，启动 500ms tick 主循环。每 tick SHALL：(1) drain UiEvent 队列；(2) poll InputService；(3) `Launcher::dispatch`（overlay > foreground > global shortcut）；(4) 快照 RenderCtx；(5) 若 `dirty || tick_elapsed` 则 render（StatusBar + foreground view + overlay）到 fb；(6) 若 `screen_on` 则 `lcd.present(fb)`。熄屏时 SHALL 跳过 render + present，但 model 状态保留。

#### Scenario: 输入触发重绘
- **WHEN** 用户触摸 Launcher 的 Settings 入口
- **THEN** hit_test 返回 SettingsTile，`launcher.launch(Settings)` 切换前台，`dirty = true`，下一 tick render Settings 首页并 present

#### Scenario: 熄屏暂停渲染
- **WHEN** 30 秒无操作触发 `screen_off()`
- **THEN** 主循环停止 render + present，model 状态保留；唤醒后 `screen_on()` + 立即重绘当前状态

### Requirement: 过程式触摸 hit-testing
各 view SHALL 实现 `hit_test(x, y) -> Option<HitTarget>`，根据当前 model 状态 + 已知布局规则计算命中目标，SHALL NOT 依赖保留的场景图。`HitTarget` SHALL 为 view 自定义的命中目标枚举（如 `IntercomTile` / `SettingsTile` / `PageNav(dir)`）。滑页手势（>40px 水平、<20px 垂直）SHALL 由 controller 层用 `Swipe{dx,dy}` 判定，SHALL NOT 进 view hit_test。

#### Scenario: 网格命中
- **WHEN** Launcher 首页 2×1 网格，触摸坐标 `(60, 120)` 落在左半区
- **THEN** `hit_test` 返回 `Some(HitTarget::IntercomTile)`

#### Scenario: 滑页不误触发 hit_test
- **WHEN** 用户在 Settings 页水平拖动 60px 后松手
- **THEN** controller 判定为滑页手势，SHALL NOT 调用 view hit_test，SHALL 切换到下一页

### Requirement: StatusBar 组件
`src/apps/view/status_bar.rs` SHALL draw 全局顶部状态区（240×20px），包含：模式图标（清晰/自由，已组网时显示）/ 静音图标 / 已组网状态 / 信号 4 格 / 电量 4 档 / 当前时间。所有图标 SHALL 用 `embedded-graphics` 原语（Rectangle/Line/Polyline）绘制，SHALL NOT 使用位图资产。数据来源 SHALL 为 `RenderCtx` 快照。

#### Scenario: 静音开启显示图标
- **WHEN** `ctx.muted == true` 且主循环渲染状态栏
- **THEN** 状态栏显示静音图标

#### Scenario: 电量档位刷新
- **WHEN** 快照 `battery_step` 从 3 变为 2
- **THEN** 下一 tick 状态栏电量图标显示 2 档

### Requirement: HalDisplayService with_fb
`HalDisplayService` SHALL 暴露 `with_fb(&self, f: impl FnOnce(&mut Rgb565Buf) -> Result<(), HalError>) -> Result<(), HalError>`，回调内允许 view 层直接绘制到 framebuffer，回调返回后自动调用 `LcdDriver::present` 推送。`DrawCmd` SHALL 新增 `Redraw` 变体。`present(DrawCmd::Redraw)` SHALL 将当前 framebuffer 原样重新推送到 LCD（SHALL NOT 重新调用 view 层 render），用于屏幕唤醒后恢复保留的画面。主循环渲染路径 SHALL 经 `with_fb` 回调绘制，SHALL NOT 经 `present(DrawCmd::Redraw)` 触发渲染。

#### Scenario: view 通过 with_fb 绘制
- **WHEN** 主循环调用 `display.with_fb(|fb| view.render(fb, &ctx))`
- **THEN** view 绘制到 fb，回调返回后 `lcd.present(fb)` 自动推送，屏幕更新

#### Scenario: 唤醒后 Redraw 恢复画面
- **WHEN** 屏幕从熄屏唤醒 `screen_on()` 后，framebuffer 内容仍保留
- **THEN** 调用 `present(&DrawCmd::Redraw)` 将当前 framebuffer 原样推送到 LCD，不重新渲染，屏幕恢复熄屏前画面

### Requirement: CJK 子集位图字体
项目 SHALL 在 Phase 4 前提供 CJK 子集位图字体（从 Noto Sans CJK 12px 抽取 spec 全部中文文案去重后的字符子集，约 100 字），生成 `mono_font` 兼容的 `MonoFont` 常量，编译进固件（flash 占用约 20-40KB）。`Text::new` SHALL 能渲染该子集内的中文字符。

#### Scenario: 中文文案可渲染
- **WHEN** IntercomView draw 群组信息退出确认模态，调用 `Text::new("退出后本机将删除当前组信息", ...).draw(fb)`
- **THEN** 屏幕显示该中文文案，无方框/乱码

#### Scenario: 子集外字符回退
- **WHEN** `Text::new` 接收子集外的中文字符
- **THEN** 该字符显示为方框或空白，SHALL NOT panic

### Requirement: Rgb565Buf DrawTarget 适配器
项目 SHALL 在 `src/services/display_buf.rs` 定义 `Rgb565Buf` 类型，持有堆分配的 `w*h*2` 字节缓冲区，SHALL 实现 `embedded_graphics::draw_target::DrawTarget<Color = Rgb565>`。像素 SHALL 以大端字节序存储（高字节在前），匹配 ST7789 16-bit SPI 数据顺序。`fill_solid` SHALL 对矩形区域批量写入以优化清屏 / 背景填充性能。`set_pixel` SHALL 对越界坐标静默忽略（SHALL NOT panic）。`as_bytes()` SHALL 返回原始字节切片供 `LcdDriver::present` 直接 DMA 推送。

#### Scenario: 缓冲区大小匹配分辨率
- **WHEN** 构造 `Rgb565Buf::new(240, 240)`
- **THEN** `as_bytes().len() == 115200`（240×240×2）

#### Scenario: 大端字节序匹配 ST7789
- **WHEN** 对 `Rgb565Buf` 调用 `fill(Rgb565::WHITE)`（0xFFFF）
- **THEN** 每个像素的字节序列为 `[0xFF, 0xFF]`，高字节在前，与 ST7789 16-bit SPI 顺序一致

#### Scenario: 越界像素写入不 panic
- **WHEN** embedded-graphics 原语绘制超出 240×240 边界的像素
- **THEN** `set_pixel` 静默忽略越界坐标，SHALL NOT panic 或返回 Err

#### Scenario: fill_solid 仅写入矩形内部
- **WHEN** 在 8×8 缓冲区上对 `(2,2,3×2)` 矩形调用 `fill_solid(RED)`
- **THEN** 矩形内部像素为红色，矩形外像素保持原值不被覆盖

### Requirement: LauncherView 2×1 应用入口网格
`src/apps/view/launcher_view.rs` SHALL 提供 `draw_launcher(fb, ctx)` 函数，在状态栏下方（y ≥ `STATUS_BAR_H`）绘制 2×1 网格：左 tile = Intercom 入口，右 tile = Settings 入口。每个 tile SHALL 包含填充背景、边框、居中图标（embedded-graphics 原语绘制，无位图资产）、图标下方居中英文标签（"Intercom" / "Settings"）。两 tile + 间隙 + 边距 SHALL 恰好填满 240px 宽度。SHALL 提供 `hit_test(x, y) -> Option<HitTarget>`：y < `STATUS_BAR_H` 返回 `None`；左 tile 区域返回 `LauncherIntercomTile`；右 tile 区域返回 `LauncherSettingsTile`。

#### Scenario: Launcher 首页渲染双入口
- **WHEN** 主循环调用 `draw_launcher(fb, ctx)` 且前台为 Launcher
- **THEN** 状态栏下方显示两个 tile，左 tile 含 Intercom 图标 + "Intercom" 标签，右 tile 含 Settings 图标 + "Settings" 标签，无花屏

#### Scenario: 触摸左 tile 命中 Intercom
- **WHEN** 触摸坐标 `(60, 120)` 落在左 tile 区域
- **THEN** `hit_test(60, 120)` 返回 `Some(HitTarget::LauncherIntercomTile)`

#### Scenario: 触摸右 tile 命中 Settings
- **WHEN** 触摸坐标 `(180, 120)` 落在右 tile 区域
- **THEN** `hit_test(180, 120)` 返回 `Some(HitTarget::LauncherSettingsTile)`

#### Scenario: 状态栏区域不命中
- **WHEN** 触摸坐标 `(120, 10)` 落在状态栏区域（y < `STATUS_BAR_H`）
- **THEN** `hit_test(120, 10)` 返回 `None`

### Requirement: Launcher App trait 实现
`Launcher` SHALL 实现 `App` trait。`id()` SHALL 返回 `"launcher"`，`title()` SHALL 返回 `"Launcher"`。`render(fb, ctx)` SHALL 委托 `view::launcher_view::draw_launcher`，`hit_test(x, y, ctx)` SHALL 委托 `view::launcher_view::hit_test`。主循环 SHALL 经 `App::render` 统一派发前台视图，SHALL NOT 对 Launcher 特判直接调用 `draw_launcher`。`Launcher` 的状态机逻辑（`dispatch_input` / `tick` / `launch` / `back`）SHALL NOT 改变——仅新增 `render` / `hit_test` 实现委托到 view 层。

#### Scenario: 主循环经 App::render 派发 Launcher
- **WHEN** foreground == Launcher 且主循环触发渲染
- **THEN** 主循环调用 `launcher.render(fb, ctx)`（而非直接调 `draw_launcher`），Launcher 委托 `draw_launcher` 绘制首页

#### Scenario: Launcher 状态机逻辑不变
- **WHEN** 为 Launcher 添加 `render` / `hit_test` 实现
- **THEN** `dispatch_input` / `tick` / `launch` / `back` 的行为与 change 07 定义一致，无回归

### Requirement: SettingsApp App::render 实现与页面分派
`SettingsApp` SHALL 实现 `App` trait。`id()` SHALL 返回 `"settings"`，`title()` SHALL 返回 `"Settings"`。`render(fb, ctx)` SHALL 按 `self.page()`（`SettingsPage` 枚举：`DeviceName` / `Volume` / `Mute` / `Brightness` / `ScreenOffTime` / `About` / `FactoryReset`）分派到 `view::settings_view::draw_<page>` 函数。`hit_test(x, y, ctx)` SHALL 按当前页布局返回命中目标——Phase 2 SHALL 返回 `None`（只读渲染，不处理触摸编辑），Phase 3 补全各页命中目标。`SettingsApp` 的状态机逻辑（`swipe_next` / `swipe_prev` / `set_volume` / `set_brightness` / `set_muted` / `set_screen_off_sec` / `set_device_name` / `factory_reset_arm` / `factory_reset_confirm` / `factory_reset_cancel`）SHALL NOT 改变——仅新增 `render` / `hit_test` 实现委托到 view 层。

#### Scenario: render 按 page 分派到对应 draw 函数
- **WHEN** `self.page() == SettingsPage::Volume` 且主循环调用 `settings_app.render(fb, ctx)`
- **THEN** 分派到 `view::settings_view::draw_volume_page(fb, ctx)`，绘制音量页

#### Scenario: Phase 2 hit_test 返回 None
- **WHEN** Phase 2 用户触摸音量页滑块区域，调用 `settings_app.hit_test(x, y, ctx)`
- **THEN** 返回 `None`，触摸编辑未被处理（Phase 3 补全命中目标）

#### Scenario: 状态机逻辑不变
- **WHEN** 为 SettingsApp 添加 `render` / `hit_test`
- **THEN** `swipe_next` / `swipe_prev` / `set_*` / `factory_reset_*` 行为与 change 07 一致，无回归

### Requirement: SettingsView 7 页只读渲染契约
`src/apps/view/settings_view.rs` SHALL 提供 7 个 draw 函数（每页一个），每函数读 `SettingsApp` model + `RenderCtx` 快照，用 `embedded-graphics` 原语（`Rectangle` / `Line` / `Text` / `MonoTextStyle`）绘制到 fb，SHALL NOT 使用位图资产。每页 SHALL 在内容区（y ≥ `STATUS_BAR_H`）绘制，SHALL NOT 覆盖状态栏。Phase 2 SHALL 只读渲染当前 model 状态，SHALL NOT 处理触摸编辑（滑块拖动持久化 / 按钮点击 / 文本输入在 Phase 3 / Phase 5 接入）。各页内容 SHALL 为：

- **设备名称页（`draw_device_name_page`）**：draw 当前 `Settings.device_name` 文本 + 编辑框占位（矩形边框）+ "Random" 按钮（英文标签）
- **系统音量页（`draw_volume_page`）**：draw 滑块轨道（0-100）+ 手柄位于 `Settings.volume` 对应位置 + 当前数值文本
- **全局静音页（`draw_mute_page`）**：draw 开关控件（ON/OFF 两态）+ 当前 `Settings.muted` 状态高亮
- **亮度页（`draw_brightness_page`）**：draw 滑块轨道（0-100）+ 手柄位于 `Settings.brightness` 对应位置 + 当前数值文本
- **自动熄屏时间页（`draw_screen_off_time_page`）**：draw 选项列表（5s / 15s / 30s / 60s / Always-on）+ 当前 `Settings.screen_off_sec` 对应项高亮
- **关于页（`draw_about_page`）**：draw 4 项诊断文本——固件版本（`ctx.fw_version`）/ 上次复位原因（`ctx.reset_reason.display()`）/ 异常重启次数（`ctx.abnormal_boot_count`）/ 安全启动标记（`ctx.safe_mode`），复用 `AboutData` 语义
- **恢复出厂页（`draw_factory_reset_page`）**：按 `FactoryResetState` 分支——`Idle` 显示警告文本 + "Confirm" 按钮；`FirstConfirm` 显示二次确认文本 + "Cancel" / "Confirm Reset" 按钮

#### Scenario: 设备名称页显示当前名称
- **WHEN** `self.page() == SettingsPage::DeviceName` 且 `Settings.device_name == "INT-0001"`
- **THEN** 设备名称页 draw "INT-0001" 文本 + 编辑框占位 + "Random" 按钮

#### Scenario: 音量页滑块反映当前值
- **WHEN** `self.page() == SettingsPage::Volume` 且 `Settings.volume == 70`
- **THEN** 音量页 draw 滑块手柄位于 70% 位置 + 显示数值 "70"

#### Scenario: 静音页开关反映状态
- **WHEN** `self.page() == SettingsPage::Mute` 且 `Settings.muted == true`
- **THEN** 静音页 draw 开关处于 ON 位置

#### Scenario: 亮度页滑块反映当前值
- **WHEN** `self.page() == SettingsPage::Brightness` 且 `Settings.brightness == 80`
- **THEN** 亮度页 draw 滑块手柄位于 80% 位置 + 显示数值 "80"

#### Scenario: 熄屏时间页高亮当前选项
- **WHEN** `self.page() == SettingsPage::ScreenOffTime` 且 `Settings.screen_off_sec == 30`
- **THEN** 熄屏时间页 draw 5 项列表，"30s" 项高亮，其余项正常显示

#### Scenario: 关于页显示 4 项诊断
- **WHEN** `self.page() == SettingsPage::About`
- **THEN** 关于页 draw 固件版本 / 上次复位原因 / 异常重启次数 / 安全启动标记 4 行文本，数据来自 `RenderCtx` 快照

#### Scenario: 恢复出厂页 Idle 态显示警告 + 确认
- **WHEN** `self.page() == SettingsPage::FactoryReset` 且 `factory_reset_state() == FactoryResetState::Idle`
- **THEN** 恢复出厂页 draw 警告文本 + "Confirm" 按钮

#### Scenario: 恢复出厂页 FirstConfirm 态显示二次确认
- **WHEN** `self.page() == SettingsPage::FactoryReset` 且 `factory_reset_state() == FactoryResetState::FirstConfirm`
- **THEN** 恢复出厂页 draw 二次确认文本 + "Cancel" / "Confirm Reset" 按钮

#### Scenario: Phase 2 不处理触摸编辑
- **WHEN** Phase 2 用户触摸音量页滑块区域
- **THEN** 触摸事件未被 view 处理（`hit_test` 返回 `None`），`Settings.volume` SHALL NOT 被修改

### Requirement: 触摸命中派发到 model 动作
主循环 SHALL 在 `Touch(Down)` 事件且非滑页手势、非首触过滤时，调用当前前台 app 的 `hit_test(x, y, ctx)`。若返回 `Some(HitTarget)`，主循环 SHALL 将命中目标转成对应 model 动作：`LauncherIntercomTile` → `launcher.launch(AppId::Intercom)`；`LauncherSettingsTile` → `launcher.launch(AppId::Settings)`；`SettingsControl{field}` → 调用 `SettingsApp::set_*` 对应字段；`IntercomPttArea` → PTT 派发；`VolumeMuteBtn` → toggle `Settings.muted`；`VolumePanelClose` → 关闭 overlay。若返回 `None`，SHALL 不修改 model。命中派发后 SHALL 置 `dirty = true`。

#### Scenario: 触摸 Launcher 的 Settings 入口切换前台
- **WHEN** 前台为 Launcher，用户 `Touch(Down{60,120})`，`hit_test` 返回 `LauncherSettingsTile`
- **THEN** 主循环调用 `launcher.launch(AppId::Settings)`，前台切到 Settings，`dirty = true`

#### Scenario: 命中背景不修改 model
- **WHEN** `Touch(Down{120,5})` 命中状态栏区域，`hit_test` 返回 `None`
- **THEN** 主循环 SHALL 不修改任何 model 状态

### Requirement: SettingsView 各页 hit_test 目标
`SettingsView` SHALL 在 Phase 3 补全各页 `hit_test` 返回值（Phase 2 的 `None` 在 Phase 3 替换为真实目标）。各页命中目标 SHALL 为：

- **设备名称页**：编辑框区域 → `SettingsControl{field: DeviceName}`；"Random" 按钮区域 → `SettingsControl{field: DeviceNameRandom}`
- **系统音量页**：滑块轨道区域 → `SettingsControl{field: Volume}`
- **全局静音页**：开关区域 → `SettingsControl{field: Mute}`
- **亮度页**：滑块轨道区域 → `SettingsControl{field: Brightness}`
- **自动熄屏时间页**：各选项行区域 → `SettingsControl{field: ScreenOffTime(option)}`（option 标识 5/15/30/60/AlwaysOn）
- **关于页**：无命中目标（只读，返回 `None`）
- **恢复出厂页**：Idle 态 "Confirm" 按钮区域 → `SettingsControl{field: FactoryResetArm}`；FirstConfirm 态 "Cancel" 区域 → `SettingsControl{field: FactoryResetCancel}`，"Confirm Reset" 区域 → `SettingsControl{field: FactoryResetConfirm}`

#### Scenario: 音量页滑块命中
- **WHEN** `self.page() == Volume` 且 `Touch(Down)` 落在滑块轨道区域
- **THEN** `hit_test` 返回 `Some(SettingsControl{field: Volume})`

#### Scenario: 关于页无命中
- **WHEN** `self.page() == About` 且 `Touch(Down)` 在内容区任意位置
- **THEN** `hit_test` 返回 `None`（只读页）

#### Scenario: 恢复出厂页 Confirm Reset 命中
- **WHEN** `self.page() == FactoryReset` 且 `factory_reset_state() == FirstConfirm` 且 `Touch(Down)` 落在 "Confirm Reset" 按钮区域
- **THEN** `hit_test` 返回 `Some(SettingsControl{field: FactoryResetConfirm})`

### Requirement: 滑页手势 + 熄屏计时 + 首触过滤接通主循环
主循环 SHALL 在 `Touch(Up)` 或 `Touch(Swipe{dx,dy})` 事件判定滑页手势：`|dx| > 40` 且 `|dy| < 20` SHALL 视为滑页，左滑（`dx < 0`）→ `settings_app.swipe_next()` 或 `intercom_app` 下一页，右滑（`dx > 0`）→ `swipe_prev()`；SHALL NOT 调用 view `hit_test`。滑页后 SHALL 置 `dirty = true`。

主循环 SHALL 维护熄屏计时：每秒 tick 累加 `last_activity_tick`，达到 `Settings.screen_off_sec` 时调用 `display.screen_off()`；任一用户交互（Touch / BootPress / BootShortTap / PowerShortPress / PlusShortPress）SHALL 重置计时。熄屏期间收到 `Touch(Down)` / `PowerShortPress` / `BootShortTap` 唤醒源时 SHALL 仅调 `display.screen_on()` 并立即重绘当前 model 状态，SHALL NOT 将首触转发给 view `hit_test` 或 model 动作；唤醒后第二次触摸起正常转发。

#### Scenario: Settings 左滑切换下一页
- **WHEN** 前台为 Settings 且 `Touch(Swipe{dx:-60, dy:5})`
- **THEN** 主循环判定为左滑手势，调用 `settings_app.swipe_next()`，`dirty = true`，SHALL NOT 调用 view `hit_test`

#### Scenario: 垂直滑动不触发滑页
- **WHEN** `Touch(Swipe{dx:10, dy:50})`
- **THEN** 不判定为滑页手势（`|dy| > 20`），SHALL 走 `hit_test` 路径

#### Scenario: 30 秒无操作熄屏
- **WHEN** `Settings.screen_off_sec == 30` 且 30 秒内无用户交互
- **THEN** 主循环调用 `display.screen_off()`，后续 tick SHALL 跳过 render + present

#### Scenario: 熄屏首触只唤醒不转发
- **WHEN** 屏幕熄屏且用户 `Touch(Down)` 触摸
- **THEN** 主循环调用 `display.screen_on()` + 立即重绘当前 model，该 `Touch(Down)` SHALL NOT 转发给 view `hit_test`，SHALL NOT 触发任何按钮点击

#### Scenario: 唤醒后第二次触摸正常操作
- **WHEN** 唤醒亮屏后用户进行第二次 `Touch(Down)`
- **THEN** 该事件正常转发给前台 view `hit_test`，触发对应交互

#### Scenario: 用户交互重置熄屏计时
- **WHEN** 熄屏计时未到期前发生任一用户交互
- **THEN** `last_activity_tick` 重置为 0，重新计时

### Requirement: IntercomApp App::render 实现与状态分派
`IntercomApp` SHALL 实现 `App` trait。`id()` SHALL 返回 `"intercom"`，`title()` SHALL 返回 `"Intercom"`。`render(fb, ctx)` SHALL 按当前 `IntercomState`（未组网 / 已组网）+ `IntercomUiState`（Idle / Listening / PttArming / PttActive / ChannelBusy）+ 当前 intercom page index 分派到 `view::intercom_view::draw_<page>` 函数。`hit_test(x, y, ctx)` SHALL 返回 `IntercomPttArea`（底部 PTT 区命中）或 `IntercomPageNav{forward: bool}`（左右滑页区命中）或 `None`。`IntercomApp` 的状态机逻辑（`dispatch` / `dispatch_voice` / `on_peer_voice_state` / `update_peers` / `set_mode`）SHALL NOT 改变——仅新增 `render` / `hit_test` 实现委托到 view 层。

#### Scenario: render 按状态分派
- **WHEN** 设备未组网且主循环调用 `intercom_app.render(fb, ctx)`
- **THEN** 分派到 `draw_unjoined_page(fb, ctx)`，绘制未组网页

#### Scenario: 已组网主对讲页分派
- **WHEN** 设备已组网且 `ui_state == Idle` 且 intercom page == 主对讲
- **THEN** 分派到 `draw_main_talk_page(fb, ctx)`，按 peer 数自适应布局

#### Scenario: 状态机逻辑不变
- **WHEN** 为 IntercomApp 添加 render / hit_test
- **THEN** `dispatch` / `dispatch_voice` / `on_peer_voice_state` / `update_peers` 行为与 change 13 一致

### Requirement: IntercomView 4 页渲染契约
`src/apps/view/intercom_view.rs` SHALL 提供 draw 函数，读 `IntercomApp` model + `RenderCtx` 快照，用 `embedded-graphics` 原语绘制到 fb。各页内容 SHALL 为：

- **未组网页（`draw_unjoined_page`）**：draw "Create Host" 入口 + 搜索主机列表（每行 6 字段：主机名 / MAC 后 4 位 / 信号 4 格 / 当前人数·上限 / 模式标签 / 可加入图标）+ 加入流程页（目标主机模式 / 成员列表 / 人数 / 自己加入状态 / 当前信号 / 退出入口）
- **主对讲页（`draw_main_talk_page`）**：按在线 peer 数自适应布局——1=单大卡、2=双卡、3=三分布局、4=四宫格；每卡 draw 名称 + 在线/离线图标 + 信号 4 格 + 发言图标；底部 SHALL 保留大面积 PTT 区；`ChannelBusy` 状态下 PTT 区边框 SHALL 灰色降权显示
- **变声器页（`draw_voice_changer_page`）**：draw 3 档选项（Normal / PitchUp / PitchDown）+ 预览入口 + 当前模式高亮
- **群组信息·退出页（`draw_group_info_page`）**：draw 组信息（组名 / 模式 / 成员 / 信道）+ "Exit Group" 按钮 → 二次确认模态（中文文案"退出后本机将删除当前组信息"）

未组网时只渲染未组网页；已组网时主对讲 / 变声器 / 群组信息三页 SHALL 支持左右滑切换。所有中文文案 SHALL 使用 CJK 子集字体（见 CJK Requirement），SHALL NOT 显示方框/乱码。

#### Scenario: 未组网页显示创建主机 + 搜索列表
- **WHEN** 设备未组网且 `draw_unjoined_page(fb, ctx)` 被调用
- **THEN** 渲染 "Create Host" 入口 + 搜索列表（每行 6 字段完整显示）

#### Scenario: 主对讲页单 peer 单大卡
- **WHEN** 已组网且在线 peer 数 == 1
- **THEN** 主对讲页 draw 单张大卡片占满内容区，含名称 + 在线图标 + 信号 + 发声图标

#### Scenario: 主对讲页 4 peer 四宫格
- **WHEN** 已组网且在线 peer 数 == 4
- **THEN** 主对讲页 draw 2×2 四宫格，每卡含相同字段，无 MAC 后 4 位

#### Scenario: ChannelBusy 时 PTT 区降权
- **WHEN** `ui_state == ChannelBusy` 且渲染主对讲页
- **THEN** 底部 PTT 区边框 draw 为灰色，视觉降权

#### Scenario: 群组退出二次确认中文渲染
- **WHEN** 用户点击 "Exit Group" 触发二次确认模态
- **THEN** 模态 draw "退出后本机将删除当前组信息" 中文文案，使用 CJK 子集字体，无方框/乱码

#### Scenario: 变声器页当前模式高亮
- **WHEN** `IntercomApp.mode == PitchUp` 且渲染变声器页
- **THEN** "PitchUp" 选项高亮显示

### Requirement: IntercomEvent 跨线程 UiEvent 投递
ESP-NOW 接收线程 / 音频线程 SHALL 在收到事件时调用 `push_ui_event(queue, UiEvent::Intercom(ev))`（或 `Network` / `Audio` 变体）。主循环 SHALL 每 tick `drain_ui_events` 后，将 `UiEvent::Intercom(ev)` 派发到 `IntercomApp::dispatch_voice` 或 `on_peer_voice_state` / `update_peers`，更新 peer 卡片状态。drain 后 SHALL 置 `dirty = true` 触发重绘，确保 IntercomEvent 投递后屏幕立即刷新（无 ms 级延迟可见）。占位变体（`IntercomEvent = ()` 未交付时）SHALL 记日志并忽略，SHALL NOT panic。

#### Scenario: espnow 回调投递 PeerOnline
- **WHEN** ESP-NOW 接收线程收到 `PeerOnline(id, rssi_4)` 事件
- **THEN** `push_ui_event(queue, UiEvent::Intercom(PeerOnline))` 被调用，主循环下一 tick drain 后更新 IntercomApp peer 卡片 + 置 dirty 重绘

#### Scenario: drain 后立即重绘
- **WHEN** UiEvent 队列有事件且主循环 drain 完成
- **THEN** `dirty = true`，当前 tick 内 SHALL 完成 render + present，peer 卡片状态在屏幕上立即刷新

#### Scenario: 占位变体不 panic
- **WHEN** `IntercomEvent` 类型未交付，主循环 drain 到 `UiEvent::Intercom(())` 占位值
- **THEN** 记日志并忽略，SHALL NOT panic，等对应 change 交付后替换为真实类型

### Requirement: VolumePanel overlay 渲染契约
`src/apps/view/volume_panel.rs` SHALL 提供 `draw_volume_panel(fb, ctx, settings)` 函数，当 `Launcher.overlay() == Some(Overlay::VolumePanel)` 时由主循环在前景 view 之上绘制。SHALL draw 半透明遮罩覆盖全屏（降低前景可见度）+ 居中音量滑块（0-100，手柄位于 `Settings.volume`）+ 静音按钮（反映 `Settings.muted`）。SHALL 提供 `hit_test(x, y) -> Option<HitTarget>`：静音按钮区域 → `VolumeMuteBtn`；遮罩区域（非滑块/非按钮）→ `VolumePanelClose`。PLUS 短按 SHALL 切换 overlay 开/关（app-shell 已定义，Phase 5 接通渲染）。

#### Scenario: 音量面板 overlay 渲染
- **WHEN** `overlay == Some(VolumePanel)` 且 `Settings.volume == 50`
- **THEN** 主循环 draw 前景 view 后叠加 draw 半透明遮罩 + 居中滑块手柄位于 50% + 静音按钮

#### Scenario: 点击静音按钮
- **WHEN** `Touch(Down)` 落在静音按钮区域，`hit_test` 返回 `VolumeMuteBtn`
- **THEN** 主循环 toggle `Settings.muted` + `save_settings` + `dirty = true`

#### Scenario: 点击遮罩关闭面板
- **WHEN** `Touch(Down)` 落在遮罩区域（非滑块/非按钮），`hit_test` 返回 `VolumePanelClose`
- **THEN** 主循环 `overlay = None` + `dirty = true`

### Requirement: 滑块拖动即时持久化
Phase 5 SHALL 接通滑块拖动交互（Phase 3 仅命中单点，Phase 5 补连续拖动）。`Touch(Down)` 命中滑块后，后续 `Touch(Move)` 或连续 `Touch(Down)` 事件 SHALL 实时更新对应字段并持久化：

- **音量滑块**：拖动 → `settings_app.set_volume(new_v)` → `save_settings` 持久化 + `dirty = true`
- **亮度滑块**：拖动 → `settings_app.set_brightness(new_v)` → `DisplayService::set_brightness(new_v)` 即时生效 AND `save_settings` 持久化 + `dirty = true`
- **音量面板 overlay 滑块**：拖动 → 同音量滑块逻辑

字段值 SHALL 按滑块手柄 x 坐标映射到 0-100 区间并 clamp。拖动结束（`Touch(Up)`）SHALL NOT 回退值。

#### Scenario: 音量滑块拖动持久化
- **WHEN** 用户在音量页拖动滑块从 50 到 70
- **THEN** `set_volume(70)` 被调用，`save_settings` 持久化，状态栏/滑块手柄即时反映 70

#### Scenario: 亮度滑块拖动即时生效
- **WHEN** 用户在亮度页拖动滑块从 80 到 50
- **THEN** `set_brightness(50)` 被调用，`DisplayService::set_brightness(50)` 即时调暗背光，`save_settings` 持久化

#### Scenario: 滑块值 clamp 到 0-100
- **WHEN** 拖动手柄超出轨道右边界
- **THEN** 字段值 clamp 为 100，SHALL NOT 超出

### Requirement: 熄屏对讲保持 controller 接通
主循环 SHALL 在熄屏状态下保持 `IntercomState` 不变，对讲收发链路 SHALL 继续运行。熄屏时收到他人语音 SHALL 正常播放且 SHALL NOT 亮屏、SHALL NOT 重绘。熄屏时按 PTT（`BootPress`）SHALL 直接派发到 `IntercomApp::dispatch` 进入 `PttActive` 发言，SHALL NOT 调 `display.screen_on()`、SHALL NOT 重绘。唤醒亮屏后主循环 SHALL 立即重绘当前 IntercomApp 状态（model 即 retained state，无 slint Window 需保留）。

#### Scenario: 熄屏收到语音不亮屏
- **WHEN** 屏幕熄屏且 `IntercomState == Grouped(Idle)`，收到他人语音帧
- **THEN** 系统进入 `Grouped(Listening)`，音频正常播放，屏幕保持熄屏，主循环 SHALL NOT 调 `screen_on()` 或 render

#### Scenario: 熄屏按 PTT 直接发言不亮屏
- **WHEN** 屏幕熄屏且 `IntercomState == Grouped(Idle)`，用户按下 BOOT
- **THEN** 主循环派发到 `intercom_app.dispatch(BootPress)` 进入 `PttActive`，屏幕保持熄屏，SHALL NOT 调 `screen_on()`

#### Scenario: 唤醒后立即重绘对讲状态
- **WHEN** 屏幕从熄屏唤醒 `screen_on()`
- **THEN** 主循环立即 render 当前 IntercomApp 状态（如 `Listening`）+ present，model 状态保留无需重建

### Requirement: 非对讲低功耗待机 controller 接通
主循环 SHALL 在以下四条件同时满足时调用 `PowerService::enter_standby()`：(1) `IntercomState` 为未组网或 `Grouped(Idle)`；(2) 无前台持续任务（无组队进行中、无 VolumePanel overlay 弹出）；(3) 无用户交互超过 `STANDBY_GRACE_SEC`（默认 60 秒）；(4) 音频采集与播放均停止。任一条件失效时 SHALL 调用 `PowerService::wakeup()` 退出待机。待机唤醒后 SHALL 立即重绘当前 model 状态。

#### Scenario: 四条件全满足进入待机
- **WHEN** 设备未组网、无前台任务、60 秒无用户交互、音频停止
- **THEN** 主循环调用 `PowerService::enter_standby()`

#### Scenario: 任一条件失效退出待机
- **WHEN** 待机中发生用户交互或收到语音或开始组队
- **THEN** 主循环调用 `PowerService::wakeup()`，条件 (1)/(2)/(3)/(4) 任一失效即触发

#### Scenario: 待机唤醒后重绘
- **WHEN** 待机被唤醒
- **THEN** 主循环立即 render 当前 model 状态 + present，恢复交互

### 跨 change 依赖（非本期 spec 范围，记录供追踪）
PA 软启动（进入 `Listening` / `Talking` 前 `AudioService::start_playback`，退出后 `stop_playback`）依赖 change 05 AudioService 交付，SHALL 在 change 05 落地后由该 change 接通，本期 `ui-render-layer` SHALL NOT 实现 PA 软启动逻辑。
