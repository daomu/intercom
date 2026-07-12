## 1. Phase 1 — 渲染骨架 + Launcher 首页 + 状态栏（静态）

- [x] 1.1 新增 `src/apps/view/mod.rs` 聚合 view 子模块
- [x] 1.2 在 `src/apps/mod.rs` 扩展 `App` trait：新增 `render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx)` 与 `hit_test(&self, x: i32, y: i32, ctx: &RenderCtx) -> Option<HitTarget>` 方法（默认空实现，各 app 覆盖）
- [x] 1.3 定义 `RenderCtx<'a>` 值类型快照：battery_step / signal_bars / time_hms / is_grouped / muted / mode / fw_version / settings / safe_mode
- [x] 1.4 定义 `UiEvent` 枚举（`Intercom` / `Network` / `Audio` / `Dirty`）与 `UiEventQueue = Arc<Mutex<ArrayDeque<UiEvent, 64>>>`（或 `embassy_sync::channel::Channel`）
- [x] 1.5 定义 `HitTarget` 枚举（各 view 自定义子枚举的公共包装，或 trait 关联类型——Phase 1 定方案）
- [x] 1.6 新增 `src/apps/view/status_bar.rs`：draw 电量 4 档 / 信号 4 格 / 静音 / 已组网 / 时间 / 模式图标，全部用 Rectangle/Line 原语
- [x] 1.7 新增 `src/apps/view/launcher_view.rs`：draw 2×1 应用入口网格（Intercom / Settings 图标 + 英文标签），实现 hit_test
- [x] 1.8 修改 `src/services/display.rs`：`HalDisplayService` 暴露 `with_fb(&self, f: impl FnOnce(&mut Rgb565Buf) -> Result<(), HalError>) -> Result<(), HalError>`，回调内绘制后自动 `lcd.present`；新增 `DrawCmd::Redraw` 变体（原样重推 fb，用于唤醒恢复）
- [x] 1.9 修改 `src/main.rs`：移除 idle 循环，构造 `Launcher`（持有 `Arc<UiEventQueue>`）+ Settings/Intercom App，启动 50ms poll / 500ms render 主循环：drain UiEvent → snapshot RenderCtx（含 `mode` 字段）→ render（StatusBar + foreground view 经 `App::render` 派发 + overlay）→ present（若 screen_on）；Launcher 实现 `App` trait，`render` 委托 `draw_launcher`
- [ ] 1.10 `cargo run` 验证：屏幕显示状态栏 + Launcher 首页（Intercom/Settings 两个入口），无花屏（需硬件，本期代码+构建通过）

## 2. Phase 2 — Settings 静态渲染（7 页只读）

- [x] 2.1 新增 `src/apps/view/settings_view.rs`：按 `SettingsPage` 分支 draw 7 页
- [x] 2.2 设备名称页：draw 当前 `Settings.device_name` 文本 + 编辑框占位 + "Random" 按钮（英文）
- [x] 2.3 系统音量页：draw 滑块（0-100）+ 当前 `Settings.volume` 值
- [x] 2.4 全局静音页：draw 开关控件 + `Settings.muted` 状态
- [x] 2.5 亮度页：draw 滑块 + `Settings.brightness`
- [x] 2.6 自动熄屏时间页：draw 选项列表（5/15/30/60/常亮）+ 当前选中高亮
- [x] 2.7 关于页：draw 4 项诊断（固件版本 / 上次复位原因 / 异常重启次数 / 安全启动标记），复用 `AboutData` 语义
- [x] 2.8 恢复出厂页：draw 两步确认流程（警告文本 + 确认按钮 → 二次确认 + 取消/确认恢复）
- [x] 2.9 SettingsApp 实现 `App::render`：按 `self.page` 分派到 settings_view 的 draw 函数（render 委托 `draw_settings`，hit_test 委托 `settings_view::hit_test` 返回 None）
- [ ] 2.10 `cargo run` 验证：从 Launcher 看到 Settings 入口（静态显示，尚不能触摸进入）（需硬件）

## 3. Phase 3 — 触摸导航接通

- [x] 3.1 各 view 实现 `hit_test`：LauncherView（IntercomTile/SettingsTile）、SettingsView（按页返回按钮/滑块/选项命中目标）
- [x] 3.2 controller 层：Touch Down → 当前 foreground view `hit_test` → 转成 model 动作（如 LauncherView 命中 SettingsTile → `launcher.launch(AppId::Settings)`）
- [x] 3.3 controller 层：Touch Up / Swipe → 滑页手势判定（>40px 水平、<20px 垂直）→ 切换 settings page 或 intercom page
- [x] 3.4 接入 `TouchClassifier`（input.rs 已有）的熄屏首触过滤：唤醒首触不派发到 view hit_test
- [x] 3.5 熄屏计时：`last_activity_tick >= screen_off_sec` → `display.screen_off()`；唤醒 → `screen_on()` + 立即重绘
- [ ] 3.6 `cargo run` 验证：触摸 Launcher 的 Settings 入口 → 进入 Settings 首页；返回手势 → 回 Launcher；30s 无操作熄屏，触摸唤醒

## 4. Phase 4 — Intercom 视图 + CJK 字体

- [ ] 4.1 CJK 子集字体 spike：收集 spec 全部中文文案 → 去重 ~100 字 → 生成 `mono_font` 兼容 12px 位图 → 新增 `src/apps/view/font/cjk_subset.rs`，验证 1 个汉字能渲染
- [x] 4.2 新增 `src/apps/view/intercom_view.rs`：未组网页 draw（创建主机 / 搜索列表 6 字段 / 加入流程页）
- [x] 4.3 主对讲页：按在线 peer 数自适应布局（1=单大卡 / 2=双卡 / 3=三分 / 4=四宫格），每卡 draw 名称 + 在线/离线图标 + 信号 4 格 + 发声图标
- [x] 4.4 变声器页：draw 3 档（Normal/PitchUp/PitchDown）+ 预览入口 + 当前高亮
- [x] 4.5 群组信息·退出页：draw 组信息（组名/模式/成员/信道）+ 退出按钮 → 二次确认模态（文案"退出后本机将删除当前组信息"）
- [x] 4.6 底部触摸 PTT 区：draw 大面积 PTT 区 + ChannelBusy 灰色降权边框
- [x] 4.7 IntercomApp 实现 `App::render`：按当前 `IntercomState` + ui_state 分派到 intercom_view draw 函数
- [x] 4.8 IntercomEvent → UiEvent 投递：espnow 回调 `queue.push_back(UiEvent::Intercom(ev))`，主循环 drain 后更新 IntercomApp peer 状态 + 置 dirty
- [ ] 4.9 `cargo run` 验证：未组网设备显示未组网页；组网后显示成员卡片；PTT 区可触摸

## 5. Phase 5 — 手势 + overlay + 待机策略

- [x] 5.1 Intercom 三页左右滑切换（主对讲 ↔ 变声器 ↔ 群组信息）
- [x] 5.2 Settings 七页左右滑切换
- [x] 5.3 全局音量面板 overlay（`src/apps/view/volume_panel.rs`）：半透明遮罩 + 居中滑块 + 静音按钮 + PLUS 短按打开/关闭
- [x] 5.4 滑块拖动即时持久化（音量/亮度 → `save_settings`）
- [x] 5.5 熄屏对讲保持：熄屏时 IntercomState 不变，PTT 直接进入 Talking 不亮屏
- [x] 5.6 非对讲低功耗待机：四条件满足 → `PowerService::enter_standby()`；任一失效 → `wakeup()`
- [ ] 5.7 PA 软启动：进入 Listening/Talking 前 `start_playback`，退出后 `stop_playback`（依赖 change 05 AudioService）
- [ ] 5.8 `cargo run` 端到端验证：完整交互流程（Launcher → Settings 编辑 → Intercom 组网 → PTT → 滑页 → 熄屏 → 唤醒）
