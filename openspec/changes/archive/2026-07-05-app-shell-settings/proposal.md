## Why

17 个变更的第 7 个。change 03 已落地 `StorageService` trait + NVS 实现，change 04 已落地 `DisplayService` / `InputService` / `PowerService` trait + BSP 实现，但当前固件仍只有 `boot.slint` 占位画面，没有 App 抽象、没有 Launcher、没有设置入口、没有全局音量/静音交互。需要在本期引入 App trait + 注册表 + Launcher 调度壳，并实现 Settings App 的完整 slint UI 页面与全局输入快捷面板，使设备具备"可切换 App、可调节系统设置、可呼出音量/静音"的最小可用产品形态，为 change 13（对讲 App UI）提供可注册的前台框架。

## What Changes

- 新增 `src/apps/mod.rs`：定义 `App` trait（生命周期 `on_enter` / `on_exit` / `on_event` / `on_tick` + `id`/`title` 元数据）与 `AppRegistry`（注册、按 id 查找、枚举可见 App）
- 新增 `src/apps/launcher.rs`：Launcher 负责前台 App 切换、页面栈管理、全局顶部状态区渲染（电量 / 静音 / 已组网状态 / 时间）、`InputEvent` 派发到当前前台 App 或全局快捷处理、全局熄屏策略调度（无操作计时 → `DisplayService::screen_off`）、全局音量面板（PLUS 短按呼出）、全局静音 toggle（PLUS 长按）
- 新增 `src/apps/settings_app.rs`：Settings App 实现，包含以下 slint 页面：
  - 设备名称页（编辑 + 随机生成名称按钮）
  - 系统音量页（滑块 0-100）
  - 全局静音开关页
  - 亮度页（滑块 0-100）
  - 自动熄屏时间页（选项 5/15/30/60s 或常亮）
  - 关于页（固件版本 / 上次复位原因 / 异常重启次数 / 安全启动标记）
  - 恢复出厂页（两步确认：选择 → 再确认 → 调用 `StorageService::reset_settings` + `clear_group` + `clear_diag` → 重启）
- 新增 `ui/launcher.slint` + `ui/settings.slint` + `ui/volume_panel.slint`：对应 slint 页面文件
- Launcher 启动时从 `StorageService::load_settings` 读入 `Settings`，注入 Settings App；Settings App 修改后调用 `save_settings` 持久化
- 全局音量面板/静音 toggle 直接修改 `Settings` 并立即持久化 + 通知 `DisplayService`/`AudioService`（如已就绪）
- `main.rs` 启动流程接入 Launcher：BSP init → load_settings → load_group → 构造 `AppRegistry` 注册 Settings（与后续 Intercom App）→ `Launcher::run` 阻塞 slint 主循环

## Capabilities

### New Capabilities
- `app-shell`: App trait + 注册表 + Launcher 运行壳，负责前台 App 切换、输入事件派发、全局状态栏、全局熄屏策略、全局音量面板与全局静音 toggle 的快捷交互
- `settings-app`: Settings App 的 slint UI 页面集合（设备名称 / 音量 / 静音 / 亮度 / 熄屏时间 / 关于 / 恢复出厂），通过 `StorageService` 读写持久化

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 app-shell 与 settings-app -->

## Impact

- **代码**：实质填充 `src/apps/mod.rs`、新增 `src/apps/launcher.rs`、`src/apps/settings_app.rs`；新增 `ui/launcher.slint`、`ui/settings.slint`、`ui/volume_panel.slint`；`src/main.rs` 接入 Launcher 启动路径
- **构建**：`build.rs` 的 `slint_build::compile` 增加新 `.slint` 文件
- **依赖**：无新 crate（复用 change 01 声明的 `slint` / `esp-idf-svc` / `log` / `anyhow`）。变更依赖 = `03, 04, 05`：03 提供 `StorageService`（`Settings` / `DiagInfo` 读写），04 提供 `DisplayService` / `InputService` / `PowerService` trait + BSP 实现，05 提供 `AudioService`（音量/静音设置本期已持久化到 `Settings.volume` / `muted`，由 05 的 `AudioService` 在接入时读取生效；若 05 尚未集成，则实际音量/静音应用延迟到集成 change 17 时由 `AudioService` 读取 `Settings` 生效）
- **服务消费**：Launcher 与 Settings App 调用 change 03 的 `StorageService`（`load_settings` / `save_settings` / `reset_settings` / `load_diag` / `clear_diag` / `clear_group`）与 change 04 的 `DisplayService`（`set_brightness` / `screen_on` / `screen_off` / `is_screen_on`）、`InputService`（`on_event` 回调）、`PowerService`（`battery_step` / `reset_reason` / `abnormal_boot_count`）
- **后续变更**：change 13（intercom-app-ui）将向 `AppRegistry` 注册 Intercom App；change 15（power-screen-management）复用 Launcher 的熄屏策略骨架；change 16（safety-diagnostics）复用 About 页诊断字段
- **PRD 对齐**：覆盖 PRD §6（全局设置范围）、§7（PLUS 短按=音量面板 / PLUS 长按=静音 toggle）、§24（Launcher 仅暴露可用 App）、§25.2（Settings>关于 最小诊断）
