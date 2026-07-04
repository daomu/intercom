## Context

change 01 已建立可编译骨架与 slint 后端；change 02 已落地 BSP 驱动（ST7789/CST816/ES8311/ADC/按键）；change 03 已落地 `StorageService` trait + NVS 实现（`Settings` 结构含 `device_name` / `volume` / `muted` / `brightness` / `screen_off_sec` / `schema_ver`，`DiagInfo` 含 `abnormal_boot_cnt` / `safe_boot_flag` / `last_reset_reason`）；change 04 已落地 `DisplayService`（`set_brightness` / `screen_on` / `screen_off` / `is_screen_on`）、`InputService`（`on_event` 回调，`InputEvent` 枚举含 `PlusShortPress` / `PlusLongPress` / `BootShortTap` / `Touch` 等）、`PowerService`（`battery_step` / `reset_reason` / `abnormal_boot_count`）。

当前 `src/apps/mod.rs` 仍为空占位，`src/main.rs` 仅显示 `boot.slint` 占位画面。本期在此基础上引入 App trait + 注册表 + Launcher + Settings App，使设备从"点亮占位画面"升级为"可切换 App、可调节设置、可呼出音量/静音面板"的最小可用形态。

技术设计 §1.2 明确 `Settings App ──→ Storage Service / Display Service / Power Service`、`Launcher ──→ App 注册表 / Input Service / Display Service`。§20 文件组织给出 `src/apps/mod.rs`（App trait + 注册表）、`src/apps/launcher.rs`、`src/apps/settings_app.rs` 的位置。§15 页面层级给出对讲 App 三页结构（change 13 实现），本期 Launcher 不实现这些页面但预留页面栈机制。§16 熄屏策略由 Launcher 全局调度。PRD §6.2 列出 Settings 本期范围；§7.1 给出 PLUS 短按=音量面板、PLUS 长按=静音 toggle；§24 规定 Launcher 仅暴露真正可用的 App（本期仅 Settings，对讲 App 在 change 13 后暴露）。

## Goals / Non-Goals

**Goals:**
- 定义 `App` trait 与 `AppRegistry`，支持注册、按 id 查找、枚举可见 App、前台切换
- Launcher 运行壳：slint 主循环、前台 App 页面栈、全局顶部状态区、`InputEvent` 派发、全局熄屏计时、全局音量面板、全局静音 toggle
- Settings App 完整 slint 页面：设备名称（含随机生成）、系统音量、全局静音、亮度、自动熄屏时间、关于（固件版本/复位原因/异常重启次数/安全启动标记）、恢复出厂（两步确认）
- 所有设置变更通过 `StorageService::save_settings` 即时持久化；恢复出厂调用 `reset_settings` + `clear_group` + `clear_diag` 后重启
- `main.rs` 启动路径接入 Launcher

**Non-Goals:**
- 对讲 App UI 页面（主对讲页 / 变声器页 / 群组信息页）→ change 13
- PTT 发言交互流程（BOOT 长按作 PTT）→ change 10，本期 Launcher 仅处理 BOOT 短触=唤醒屏幕
- 真实音频音量/静音对 `AudioService` 的生效（AudioService 在 change 05 才落地）；本期仅持久化 `Settings.volume` / `muted`，并在 Launcher 内部维护状态，change 05 接入时读取这些值
- 低功耗待机策略细节（PA 控制、深度睡眠）→ change 15
- 完整诊断页（仅显示 PRD §25.2 的最小四项）→ change 16 扩展
- OTA 升级 UI → 不在本期范围
- 电池百分比精确显示（仅 4 档图标，复用 `PowerService::battery_step`）

## Decisions

### D1：App trait = 生命周期回调 + 元数据，无 slint 直接耦合
`App` trait 定义：`fn id(&self) -> &str`、`fn title(&self) -> &str`、`fn on_enter(&mut self, ctx: &AppContext)`、`fn on_exit(&mut self, ctx: &AppContext)`、`fn on_event(&mut self, ev: &InputEvent, ctx: &AppContext)`、`fn on_tick(&mut self, ctx: &AppContext)`。`AppContext` 持有 `&StorageService` / `&DisplayService` / `&PowerService` / `&Settings`（当前内存副本）引用，App 不直接持有 Service 实例。Settings App 使用注入的 `ctx.settings` 读取当前设置，SHALL NOT 在每次 `on_enter` 调用 `load_settings()`；仅 Launcher 启动时调用一次 `load_settings()` 读入 `Settings` 注入 `AppContext`。Settings App 修改设置后调用 `save_settings` 持久化并更新 `ctx.settings` 内存副本。App 不直接操作 slint 组件树；App 返回页面数据模型（`SettingsModel`），由 Launcher 的 slint 窗口绑定渲染。备选：App 直接持有 slint `Window`——耦合过紧，App 切换时需销毁/重建 Window，开销大且状态丢失，排除。

### D2：AppRegistry = Vec<Box<dyn App>>，启动期一次性注册
`AppRegistry` 内部 `Vec<Box<dyn App>>`，`main.rs` 启动时按依赖顺序 `register(Box::new(SettingsApp::new(...)))`。后续 change 13 注册 `IntercomApp`。`foreground_id: Option<String>` 标记当前前台 App。`enumerate_visible()` 返回 `PRD §24` 要求"真正可用"的 App 列表（本期仅 Settings）。备选：动态注册/卸载——固件场景无热插拔需求，增加复杂度，排除。

### D3：Launcher 页面栈 = 单层前台 + 全局覆盖层（音量面板）
Launcher 维护 `foreground: &mut dyn App` 与 `overlay: Option<Overlay>`（`Overlay` 枚举含 `VolumePanel`）。slint 主循环每 tick：先渲染前台 App 页面，再渲染 overlay（如有）。`InputEvent` 派发优先级：overlay（如音量面板打开时，PLUS 短按=调整音量 / 触摸=关闭面板）> 前台 App > Launcher 全局快捷（PLUS 短按=呼出面板 / PLUS 长按=静音 toggle / BOOT 短触=唤醒屏幕）。备选：多级页面栈（push/pop）——本期无深层菜单需求（PRD §8.1 "层级浅"），单层 + overlay 足够，排除。

### D4：全局状态栏 = slint 自定义组件，每 tick 刷新
`ui/launcher.slint` 定义 `StatusBar` 组件，绑定电量档位 / 静音图标 / 已组网状态 / 当前时间。Launcher 每 tick（约 500ms）从 `PowerService::battery_step` / `Settings.muted` / `StorageService::load_group().is_some()` 更新绑定。备选：事件驱动刷新——电量/静音变化非高频，且 slint 属性绑定机制对轮询友好，排除。

### D5：全局音量面板 = overlay 弹窗，PLUS 短按呼出，触摸外部或再次 PLUS 短按关闭
`ui/volume_panel.slint` 定义半透明遮罩 + 居中音量滑块 + 静音按钮。打开时 Launcher 设置 `overlay = Some(VolumePanel)`，关闭时 `overlay = None`。音量调整即时调用 `StorageService::save_settings` 持久化。备选：独立 slint Window——ESP32-C6 单 LCD，多 Window 切换开销大且无意义，排除。

### D6：全局静音 toggle = PLUS 长按，即时持久化 + 状态栏图标刷新
PLUS 长按事件触发 `Settings.muted = !Settings.muted`，立即 `save_settings`，更新状态栏静音图标。PRD §6.3 静音影响范围含对讲接收语音播放，但 AudioService 在 change 05 才落地，本期仅维护 `Settings.muted` 状态；change 05 接入时读取此值生效。备选：静音走独立 NVS key——`Settings.muted` 已在 `sys` 命名空间，无需额外 key，排除。

### D7：Settings 页面导航 = 左右滑页（PRD §8.1），单 `ui/settings.slint` 文件多视图
`ui/settings.slint` 定义一个 `SettingsWindow`，内部用 `currentIndex` 切换 7 个子视图（设备名称 / 音量 / 静音 / 亮度 / 熄屏时间 / 关于 / 恢复出厂）。触摸左右滑动切换 `currentIndex`。备选：每页独立 `.slint` 文件——编译期模块增多，且 slint 跨文件组件引用语法繁琐，单文件多视图更简洁，排除。

### D8：随机设备名称生成 = 形容词 + 名词组合，编译期词表
内置两个 `&[&str]` 词表（形容词 20 个 + 名词 20 个），运行时 `getrandom` 或 `esp_idf_sys::esp_random()` 取模选择，组合如 "SwiftFox" / "BraveLion"。生成后写入 `Settings.device_name` 并持久化。备选：使用 MAC 后 4 位——可读性差且暴露 MAC，排除。

### D9：恢复出厂 = 两步确认 + 清三命名空间 + 软重启
Settings > 恢复出厂页第一屏显示"恢复出厂设置"警告 + "确认"按钮；点击后进入第二屏"再次确认" + "取消"/"确认恢复"按钮。点击"确认恢复"后依次调用 `StorageService::reset_settings()` / `clear_group()` / `clear_diag()`，然后 `esp_idf_sys::esp_restart()` 软重启。重启后 NVS 全空，走首次使用路径。备选：仅清 `sys`——PRD §6.4 明确要求清系统设置 + 组信息 + 私钥/公钥总表，三命名空间全清，排除。

### D10：熄屏策略 = Launcher 全局计时器，无操作 N 秒后 `screen_off`
Launcher 维护 `last_activity_tick: u32`，每 tick 递增；任何 `InputEvent`（触摸/按键）重置为 0。当 `last_activity_tick >= Settings.screen_off_sec` 时调用 `DisplayService::screen_off()`。熄屏后 `BootShortTap` / `Touch` 唤醒（PRD §16.1）。对讲模式熄屏不影响收发（change 10/15 处理），本期仅实现基础计时。备选：由 `PowerService` 内部计时——PRD §16 熄屏策略跨 App 全局，Launcher 是唯一前台调度者，由其管理更合理，排除。

### D11：`on_tick` 频率 = 500ms，slint `Timer` 驱动
Launcher 用 `slint::Timer::start_default(SmollStratedInterval, 500ms, ...)` 触发 `on_tick`。500ms 平衡状态栏刷新需求与 CPU 占用。备选：100ms——CPU 占用上升且无可见收益；1000ms——状态栏电量/静音变化迟钝，排除。

### D12：App 切换入口 = Launcher 首页列表（本期仅 Settings 一项）
Launcher 首页（`foreground = None` 时）显示 `AppRegistry::enumerate_visible()` 列表，每项一个按钮（图标 + title）。点击切换 `foreground_id` 并调用 `on_enter`。本期仅 Settings 一项，但列表机制为 change 13 注册 Intercom App 预留。备选：硬编码 Launcher 首页——违反 §24 "架构可预留"原则，排除。

## Risks / Trade-offs

- **[slint 在 240×240 单屏上的多视图性能]** → Settings 单文件 7 视图可能增大编译期生成的 Rust 代码体积；若 flash 占用超预期，则拆分为多个 `.slint` 文件按需 `import`
- **[App trait 的 `&mut self` 与 slint 主循环单线程约束]** → Launcher 在 slint 主循环内同步调用 `on_tick` / `on_event`，不涉及跨线程；`App` trait 不要求 `Send`/`Sync`，简化实现
- **[全局音量面板 overlay 与前台 App 的触摸事件冲突]** → D3 已定义优先级：overlay 打开时吞掉前台 App 触摸事件；关闭后恢复
- **[恢复出厂后软重启的时序]** → `clear_group` 清 NVS `group` 命名空间；若 `esp_restart` 在 NVS flush 前执行，可能残留；调用后加 `std::thread::sleep(100ms)` 确保 NVS 落盘
- **[随机名称词表的 flash 占用]** → 40 个短词约 400B，可忽略
- **[Launcher 熄屏计时与 change 15 power-screen-management 的职责重叠]** → 本期实现基础计时；change 15 扩展为含 PA 控制 / 对讲模式不熄屏 / 深度待机，届时重构 Launcher 熄屏模块
- **[Settings App 修改后 AudioService 未就绪时音量/静音不生效]** → 本期仅持久化；change 05 接入 AudioService 时读取 `Settings` 生效，无返工

## Migration Plan

无既有运行时 App/Launcher 需迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译（含新 `.slint` 文件）
3. `cargo espflash flash` 烧录
4. 上电后 LCD 显示 Launcher 首页（仅 Settings 一项）
5. 点击 Settings 进入设置页，逐页验证
6. PLUS 短按验证音量面板呼出/关闭
7. PLUS 长按验证静音 toggle + 状态栏图标
8. 恢复出厂验证（两步确认 + 重启后 NVS 全空）

回滚：`git revert <commit>`，回到 `boot.slint` 占位画面状态。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
