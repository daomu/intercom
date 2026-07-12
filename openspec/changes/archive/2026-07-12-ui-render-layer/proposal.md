## Why

项目 5 个 UI spec（app-shell / intercom-app-ui / settings-app / power-screen-management / display-service）全部按 slint runtime 编写，但 slint 因 fontique/memmap2 依赖 POSIX libc（mmap/mprotect/madvise，`target_os = "espidf"` 缺失）无法在 ESP32-C6 编译（Cargo.toml:46-52 已明确弃用 runtime）。项目已选定 `embedded-graphics = "0.8"` 作为 fallback（Cargo.toml:72 注释 "fallback if slint is too heavy; used by DisplayService later"），且 2026-07-12 已接通 `Rgb565Buf` + `DrawTarget` + `LcdDriver::present` + boot screen，证明该路径可行。

当前状态：model 层（Launcher / SettingsApp / IntercomApp 纯逻辑状态机）已建好且测试通过；view/controller 层缺失——`HalDisplayService::present()` 仅支持 boot screen，主循环 idle，没有 App 渲染、没有状态栏、没有触摸导航。本期将 spec 的 slint 机制层翻译为 embedded-graphics 命令式渲染层，使设备从"显示 boot 文字"升级为"完整 Launcher + 状态栏 + Settings/Intercom 可见可交互"。

## What Changes

- **新增 capability `ui-render-layer`**：embedded-graphics 命令式渲染基础设施
  - `App` trait 扩展 `render(&self, target: &mut Rgb565Buf, ctx: &RenderCtx)` 方法（spec 行为层不变，机制层新增主动 draw 责任）
  - `RenderCtx` 值类型快照：battery_step / signal / time / is_grouped / muted / mode / fw_version / settings，渲染时只读、避免在 `Mutex` 上阻塞
  - `UiEvent` 队列（`Arc<Mutex<ArrayDeque<UiEvent, 64>>`）替代 `slint::invoke_from_event_loop`，主循环每 tick `drain()`，承载 ESP-NOW / 音频线程的跨线程 `IntercomEvent` 投递
  - 主渲染循环：event-driven dirty flag + 500ms tick 兜底（状态栏时间/电量/信号会变）；熄屏时停止 `present()`（power-screen-management spec 要求），唤醒后直接重绘当前 model 状态（无 slint Window 对象需保留，model 即 retained state）
  - 过程式触摸 hit-testing：各 view 暴露 `hit_test(x, y) -> HitTarget`，根据当前布局状态返回命中按钮；不保留场景图
- **新增 `src/apps/view/status_bar.rs`**：全局顶部状态栏组件，draw 电量 4 档 / 信号 4 格 / 静音 / 已组网 / 时间 / 模式图标，全部用 embedded-graphics 原语（Rectangle/Line）绘制，无位图资产
- **新增 `src/apps/view/launcher_view.rs`**：Launcher 首页视图，2×1 应用入口网格（Intercom / Settings），draw 图标 + 标签 + hit_test
- **新增 `src/apps/view/settings_view.rs`**：Settings 7 页视图（设备名称 / 音量 / 静音 / 亮度 / 熄屏 / 关于 / 恢复出厂），按 `SettingsPage` 分支 draw，含滑块/开关/文本/二次确认模态
- **新增 `src/apps/view/intercom_view.rs`**：Intercom 4 页视图（未组网 / 主对讲自适应卡片 / 变声器 / 群组信息退出），含成员卡片、PTT 区、模式图标、ChannelBusy 视觉降权
- **新增 `src/apps/view/volume_panel.rs`**：全局音量面板 overlay，半透明遮罩 + 居中滑块 + 静音按钮
- **新增 `src/apps/view/mod.rs`**：view 子模块聚合
- **修改 `src/services/display.rs`**：`HalDisplayService` 暴露 `with_fb(&self, f: impl FnOnce(&mut Rgb565Buf))` 让 view 层直接绘制到 framebuffer，然后调 `lcd.present`；`present(DrawCmd)` 支持新的 `Redraw` 变体（触发 view 层重绘）
- **修改 `src/main.rs`**：移除 idle 循环，构造 `AppRegistry` + 注册各 App + `Launcher`（含 `Arc<UiEventQueue>`），启动 500ms tick 主循环：drain UiEvent → dispatch → render → present
- **CJK 字体**：Phase 1-3 使用英文 + 图标骨架；Phase 4 前补一个子任务——从 Noto Sans CJK 抽 ~100 常用汉字生成 `mono_font` 兼容 12px 位图子集，编译进固件（~20-40KB flash）
- **修改 `src/services/input.rs`**：触摸 `Swipe{dx,dy}` 事件接入 view 层 hit_test 与滑页手势判定（>40px 水平、<20px 垂直）

## Capabilities

### New Capabilities
- `ui-render-layer`: embedded-graphics 命令式渲染基础设施（App trait render 扩展 / RenderCtx / UiEvent 队列 / 主渲染循环 / 过程式 hit-testing / 熄屏暂停渲染）

### Modified Capabilities
- `app-shell`: spec 中 slint 机制（`slint::invoke_from_event_loop` / slint 属性绑定 / slint Window 对象保留）替换为 embedded-graphics 等价机制（UiEvent 队列 / 命令式 draw / model 即 retained state）；行为要求（前台切换 / 输入派发优先级 / 熄屏计时 / 音量面板 / 静音 toggle）全部保留
- `display-service`: `present()` 从 no-op 升级为真实 framebuffer 推送；新增 `with_fb` 供 view 层直接绘制
- `intercom-app-ui`: slint `.slint` 页面文件替换为 `src/apps/view/intercom_view.rs` 的 Rust draw 实现；`IntercomEvent` 跨线程投递改用 UiEvent 队列
- `settings-app`: slint `.slint` 页面文件替换为 `src/apps/view/settings_view.rs` 的 Rust draw 实现
- `power-screen-management`: "熄屏时保留 slint Window 对象"改为"熄屏时停止 present()，model 状态保留"；唤醒后直接重绘

## 分阶段交付

| Phase | 范围 | 可验证产出 |
|-------|------|-----------|
| 1 | App trait + render() / RenderCtx / UiEvent 队列 / StatusBar / LauncherView / 主渲染循环 | 启动直接进 Launcher 首页 + 状态栏，静态可见 |
| 2 | SettingsView 7 页只读渲染 | 能从 Launcher 点进 Settings 看到各页内容（尚不能编辑） |
| 3 | 触摸 hit-test + 导航切换 + 熄屏/唤醒首触过滤 | 触摸可切换 app，状态栏实时刷新 |
| 4 | IntercomView 4 页（含 CJK 子集字体） | 对讲全流程可见 |
| 5 | 滑页手势 + 音量面板 overlay + 低功耗待机策略 | 全交互完成 |

每个 Phase 独立 `cargo run` 可验证。
