## 1. App trait 与注册表

- [ ] 1.1 在 `src/apps/mod.rs` 定义 `App` trait：`fn id(&self) -> &str`、`fn title(&self) -> &str`、`fn on_enter(&mut self, ctx: &AppContext)`、`fn on_exit(&mut self, ctx: &AppContext)`、`fn on_event(&mut self, ev: &InputEvent, ctx: &AppContext)`、`fn on_tick(&mut self, ctx: &AppContext)`
- [ ] 1.2 定义 `AppContext` 结构：持有 `&dyn StorageService` / `&dyn DisplayService` / `&dyn PowerService` / `&Settings`（当前内存副本）等引用
- [ ] 1.3 定义 `AppRegistry`：内部 `Vec<Box<dyn App>>`，实现 `register` / `find` / `enumerate_visible` / `set_foreground` / `foreground_mut`
- [ ] 1.4 定义 `Overlay` 枚举：`None` / `VolumePanel`

## 2. Launcher 框架

- [ ] 2.1 新增 `src/apps/launcher.rs`：定义 `Launcher` 结构，持有 `AppRegistry` / `Overlay` / `last_activity_tick` / `Settings` 内存副本 / Service 引用
- [ ] 2.2 实现 `Launcher::run`：初始化 slint `Timer`（500ms tick），构造 slint 主窗口，阻塞 `.run()`
- [ ] 2.3 实现 `Launcher::dispatch_event`：按优先级派发 `InputEvent`——overlay > 前台 App > 全局快捷（PLUS 短按=面板 / PLUS 长按=静音 / BOOT 短触=唤醒）
- [ ] 2.4 实现 `Launcher::on_tick`：递增 `last_activity_tick`、刷新状态栏绑定（电量 / 静音 / 已组网）、检查熄屏阈值、调用前台 App `on_tick`
- [ ] 2.5 实现 `Launcher::switch_foreground`：调用旧 App `on_exit` + 新 App `on_enter`，更新 `foreground_id`

## 3. 全局状态栏与音量面板

- [ ] 3.1 新增 `ui/launcher.slint`：定义 `StatusBar` 组件（电量图标 / 静音图标 / 已组网状态 / 时间），绑定 Launcher 暴露的属性
- [ ] 3.2 新增 `ui/volume_panel.slint`：半透明遮罩 + 居中音量滑块（0-100）+ 静音按钮
- [ ] 3.3 实现 Launcher 音量面板 overlay 逻辑：PLUS 短按打开 / 关闭、触摸遮罩关闭、滑块拖动即时 `save_settings`
- [ ] 3.4 实现 PLUS 长按静音 toggle：取反 `Settings.muted` → `save_settings` → 状态栏图标刷新

## 4. 全局熄屏策略

- [ ] 4.1 实现 `last_activity_tick` 计数器：每 tick 递增，任何 `InputEvent` 重置为 0
- [ ] 4.2 实现熄屏判定：`last_activity_tick >= screen_off_sec` → `DisplayService::screen_off()`；`screen_off_sec == u32::MAX` 时永不熄屏
- [ ] 4.3 实现唤醒逻辑：熄屏后 `BootShortTap` / `Touch` → `screen_on()`，且本次事件 SHALL NOT 派发到前台 App（吞掉首触）

## 5. Settings App 框架

- [ ] 5.1 新增 `src/apps/settings_app.rs`：定义 `SettingsApp` 实现 `App` trait，`id = "settings"`，`title = "Settings"`
- [ ] 5.2 新增 `ui/settings.slint`：定义 `SettingsWindow`，内部 `currentIndex` 切换 7 个子视图，左右滑动导航
- [ ] 5.3 实现 `SettingsApp::on_enter`：从 `AppContext` 注入的 `ctx.settings`（`&Settings` 内存副本）读取当前值填充 slint 绑定模型；SHALL NOT 调用 `load_settings()`。仅 Launcher 启动时调用一次 `load_settings()` 读入 `Settings` 注入 `AppContext`
- [ ] 5.4 实现各设置页修改后即时调用 `StorageService::save_settings` 持久化 AND 更新 `ctx.settings` 内存副本（使 Launcher 状态栏 / 后续 `on_enter` 读取最新值）

## 6. Settings 各页面

- [ ] 6.1 设备名称页：显示当前 `device_name`，编辑输入（1-16 字符），"随机生成"按钮（形容词+名词词表组合）
- [ ] 6.2 系统音量页：滑块 0-100 绑定 `Settings.volume`，调整即时持久化
- [ ] 6.3 全局静音页：开关控件绑定 `Settings.muted`，切换即时持久化
- [ ] 6.4 亮度页：滑块 0-100 绑定 `Settings.brightness`，调整即时 `DisplayService::set_brightness` + 持久化
- [ ] 6.5 自动熄屏时间页：选项 5/15/30/60s + 常亮，选择即时更新 `screen_off_sec` + 持久化
- [ ] 6.6 关于页：固件版本（`CARGO_PKG_VERSION`）/ 上次复位原因（`PowerService::reset_reason` 可读映射）/ 异常重启次数（`PowerService::abnormal_boot_count`）/ 安全启动标记（`StorageService::load_diag().safe_boot_flag`）
- [ ] 6.7 恢复出厂页：两步确认（第一屏警告+确认 → 第二屏取消/确认恢复 → 调用 `reset_settings` + `clear_group` + `clear_diag` → `esp_restart`）

## 7. main.rs 启动路径接入

- [ ] 7.1 更新 `src/main.rs`：BSP init + Service 初始化后构造 `AppRegistry`，注册 `SettingsApp`
- [ ] 7.2 实现 `AppContext` 构造：注入 `StorageService` / `DisplayService` / `InputService` / `PowerService` 引用
- [ ] 7.3 注册 `InputService::on_event` 回调到 `Launcher::dispatch_event`
- [ ] 7.4 构造 `Launcher` 并调用 `Launcher::run` 阻塞 slint 主循环
- [ ] 7.5 实现安全启动模式判定：`abnormal_boot_count` 超阈值时仅注册 Settings App

## 8. build.rs 更新

- [ ] 8.1 在 `build.rs` 的 `slint_build::compile` 调用中增加 `ui/launcher.slint` / `ui/settings.slint` / `ui/volume_panel.slint`
- [ ] 8.2 验证 slint 编译期生成对应 Rust 绑定模块路径与 `launcher.rs` / `settings_app.rs` 的 `use` 路径一致

## 9. 构建验证

- [ ] 9.1 执行 `cargo build`，确认零编译错误（含新 `.slint` 文件）
- [ ] 9.2 执行 `cargo build --release`，确认 release profile 通过
- [ ] 9.3 确认 flash 占用未超 16MB 分区上限（`ota_0` 7MB）

## 10. 烧录验收

- [ ] 10.1 连接 Waveshare ESP32-C6-Touch-LCD-1.54，执行 `cargo espflash flash --release`
- [ ] 10.2 上电后确认：LCD 显示 Launcher 首页，列表仅含 Settings 一项
- [ ] 10.3 触摸进入 Settings，逐页切换：设备名称 / 音量 / 静音 / 亮度 / 熄屏时间 / 关于 / 恢复出厂
- [ ] 10.4 验证设备名称"随机生成"按钮：生成组合名称并持久化（重启后仍在）
- [ ] 10.5 验证音量滑块：调整后重启恢复
- [ ] 10.6 验证静音开关：切换后状态栏图标刷新 + 重启恢复
- [ ] 10.7 验证亮度滑块：调整时 LCD 即时变暗/变亮 + 重启恢复
- [ ] 10.8 验证熄屏时间：设为 5 秒后 5 秒无操作熄屏；设为常亮后不熄屏
- [ ] 10.9 验证关于页：显示固件版本 / 复位原因 / 异常重启次数 / 安全启动标记四项
- [ ] 10.10 验证 PLUS 短按：呼出音量面板，再次 PLUS 短按关闭，拖动滑块持久化
- [ ] 10.11 验证 PLUS 长按：静音 toggle + 状态栏图标刷新
- [ ] 10.12 验证恢复出厂：两步确认 → 重启 → NVS 全空 → 首次使用路径（Launcher 首页、默认设置）
- [ ] 10.13 通过串口监视器确认 Launcher 启动日志包含 "Launcher started, foreground=settings" 或等价信息

## 11. 收尾

- [ ] 11.1 在仓库根目录提交 commit：`feat: app shell + launcher + settings app (change 07/17)`
- [ ] 11.2 在 commit message 注明 change 13 将向 `AppRegistry` 注册 Intercom App，change 05 将接入 AudioService 使音量/静音生效
