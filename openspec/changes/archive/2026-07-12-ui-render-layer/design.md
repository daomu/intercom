## Context

5 个 UI spec（app-shell / intercom-app-ui / settings-app / power-screen-management / display-service）于 change 04/07/13/15 编写时假设 slint runtime 可用：`slint::invoke_from_event_loop` 跨线程投递、`.slint` 声明式页面、slint 属性绑定驱动重绘、熄屏时保留 slint Window 对象。但 slint 的传递依赖 fontique/memmap2 需要 POSIX libc 的 mmap/mprotect/madvise/msync，`target_os = "espidf"` 不提供，且 fontique 假设 64-bit atomics（ESP32-C6 为 32-bit RISC-V），故 slint runtime 在 ESP-IDF 无法编译。Cargo.toml:46-52 已弃用 slint runtime，改用 `embedded-graphics = "0.8"`（注释 "fallback if slint is too heavy; used by DisplayService later"）。

2026-07-12 已接通 `Rgb565Buf`（`src/services/display_buf.rs`，实现 `embedded_graphics::DrawTarget<Color=Rgb565>`，大端字节序，ST7789 兼容）、`HalDisplayService`（owns lcd+fb，`draw_boot_screen()` 推送启动画面）、`LcdDriver::present()`（0x2C RAMWR + 4KB 分块 DMA 推送，已修复花屏）。model 层（Launcher / SettingsApp / IntercomApp）作为纯逻辑状态机在 change 07/13 已建好且测试通过，但无 view 层接驳，main.rs 仍 idle。

本期不重写 model 层（状态机行为正确），而是补 view + controller 层，把 spec 的 slint 机制翻译为 embedded-graphics 等价机制。技术设计 §1.2 的 service 依赖关系不变；§20 文件组织在 `src/apps/view/` 下新增 view 子模块。

## Goals / Non-Goals

**Goals:**
- 定义 `App` trait 的 `render()` 扩展与 `RenderCtx` 快照，使命令式渲染可测试
- `UiEvent` 队列替代 `slint::invoke_from_event_loop`，承载跨线程 IntercomEvent
- 主渲染循环（event-driven dirty + 500ms tick 兜底），熄屏暂停 present
- StatusBar / LauncherView / SettingsView / IntercomView / VolumePanel 五个 view 组件，全部用 embedded-graphics 原语 draw
- 过程式 hit-testing，无场景图
- CJK 子集位图字体（Phase 4 前补），支持 spec 要求的中文文案

**Non-Goals:**
- 重写 model 层状态机（Launcher/IntercomApp/SettingsApp 行为已正确，仅接驳 view）
- 引入 slint runtime（已确认 espidf 不可用）
- OTA 升级 UI、完整诊断页扩展（change 16 范围）
- 真音频音量/静音对 AudioService 生效（change 05 范围，本期仅持久化 + 状态栏图标）
- 低功耗深度睡眠策略细节（change 15 范围）

## Design

### MVC 分层

```
controller (main loop, 500ms tick)
  │
  ├── drain UiEvent queue (跨线程: espnow/音频回调投递)
  ├── InputService poll (触摸/按键 → InputEvent)
  ├── Launcher::dispatch (overlay > foreground > global shortcut)
  │     └── app.on_event / app.on_tick  (model 更新)
  ├── snapshot RenderCtx (从 service 读 battery/signal/time/...)
  ├── if dirty || tick_elapsed:
  │     ├── StatusBar::draw(fb, ctx)
  │     ├── foreground view::draw(fb, ctx)
  │     └── if overlay: overlay::draw(fb, ctx)
  ├── if screen_on: lcd.present(fb)
  └── sleep until next event/tick
```

model 层（apps/shell.rs / settings.rs / intercom_app.rs）不变；view 层（apps/view/）只读 model 状态 + RenderCtx 快照，绘制到 Rgb565Buf；controller 主循环驱动 drain→dispatch→render→present。

### App trait 扩展

spec 的 `App` trait（change 07）只有 `id/title/on_enter/on_exit/on_event/on_tick`——slint 声明式重绘不需要 app 主动 draw。embedded-graphics 命令式渲染要求 app 主动画自己。扩展 `App` trait：

```rust
pub trait App: Send + fmt::Debug {
    fn id(&self) -> &str;
    fn title(&self) -> &str;
    fn on_enter(&mut self, ctx: &AppContext);
    fn on_exit(&mut self, ctx: &AppContext);
    fn on_event(&mut self, ev: &InputEvent, ctx: &AppContext);
    fn on_tick(&mut self, ctx: &AppContext);
    // 新增：命令式渲染。view 读 model 自身 state + ctx 快照，draw 到 fb。
    fn render(&self, fb: &mut Rgb565Buf, ctx: &RenderCtx);
    // 新增：过程式触摸命中测试，返回点击目标（view 自定义枚举）。
    fn hit_test(&self, x: i32, y: i32, ctx: &RenderCtx) -> Option<HitTarget>;
}
```

`HitTarget` 是 view 自定义的命中目标枚举（如 LauncherView 的 `IntercomTile` / `SettingsTile`），由 controller 转回 `InputEvent` 或直接调用 model 方法。model/view 解耦但同 app 内聚——一个 app 的 state 自己画、自己 hit_test 最自然。

### RenderCtx 快照

view 渲染需要"环境数据"（电量/信号/时间/组网/静音/模式/版本）。直接 lock service Mutex 会阻塞渲染线程。每 tick 主循环快照一次到值类型：

```rust
pub struct RenderCtx<'a> {
    pub battery_step: u8,        // 0-3 (PowerService)
    pub signal_bars: u8,         // 0-4 (RadioDriver rssi → 4 档)
    pub time_hms: (u8, u8, u8),  // 本地时间 (RTC/sntp)
    pub is_grouped: bool,        // StorageService::load_group().is_some()
    pub muted: bool,             // Settings.muted
    pub mode: Option<IntercomMode>, // 已组网时模式
    pub fw_version: &'static str,
    pub settings: &'a Settings,
    pub safe_mode: bool,        // DiagInfo.safe_boot_flag
}
```

view 只读快照，service lock 仅在快照时短暂持有。

### UiEvent 队列（替代 invoke_from_event_loop）

spec 要求 IntercomEvent 回调跨线程投递到 UI。无 slint 则自建：

```rust
pub type UiEventQueue = Arc<Mutex<ArrayDeque<UiEvent, 64>>>;

pub enum UiEvent {
    Intercom(IntercomEvent),
    Network(NetworkEvent),
    Audio(AudioEvent),
    Dirty,  // 通用重绘请求
}
```

ESP-NOW 回调 / 音频线程 `queue.push_back(ev)`；主循环 `while let Some(ev) = queue.pop_front() { dispatch(ev) }`。`ArrayDeque` 来自 `arraydeque` crate（no_std 友好），或用 `embassy_sync::channel::Channel`（Cargo.toml:43 已启用 embassy-sync）。容量 64 足够（事件消费速度 > 生产速度）。

### 主渲染循环与刷新模型

混合刷新：event-driven dirty flag + 500ms tick 兜底。
- 任何 InputEvent / UiEvent 改变 model → 置 `dirty = true`
- 500ms tick：刷新 RenderCtx 快照（时间/电量/信号会变），置 `dirty = true`
- `dirty` 时：render 全屏（240×240×2 = 115200 字节，DMA 推送 <5ms，可接受全量重绘；局部脏矩形优化为后续 Non-Goal）
- 熄屏（`!screen_on`）：跳过 render + present（power-screen-management spec 要求"熄屏暂停渲染"），但 model 状态保留；唤醒后直接重绘当前状态

### 过程式 hit-testing

无场景图。每个 view 根据当前 model 状态 + 已知布局规则计算命中：

```rust
// LauncherView hit_test 示例
fn hit_test(&self, x: i32, y: i32, ctx: &RenderCtx) -> Option<HitTarget> {
    let (tile_w, tile_h) = (120, 200);
    let offset_y = 40; // 状态栏下方
    if y < offset_y { return None; } // 状态栏区
    let col = x / tile_w;
    if col == 0 && ctx.has_intercom { return Some(HitTarget::IntercomTile); }
    if col == 1 { return Some(HitTarget::SettingsTile); }
    None
}
```

240×240 屏幕 + 网格/列表布局，过程式命中简单且零内存开销。滑页手势由 controller 层用 `Swipe{dx,dy}` 判定（>40px 水平、<20px 垂直），不进 view hit_test。

### 字节序与颜色

`Rgb565Buf`（display_buf.rs）已实现大端写像素（`(v>>8, v&0xFF)`），匹配 ST7789 16-bit SPI 顺序。MADCTL=0x00（lcd.rs:99）正常方向 RGB 顺序。若现场出现颜色红蓝互换，调 MADCTL bit3（BGR）；若镜像/翻转，调 bit6/bit7（MX/MY）。boot screen 已验证文字方向正确。

### CJK 子集位图字体

Phase 1-3 使用英文 + 图标骨架（"Intercom" / "Settings" / "Exit" / "Clear" / "Free"）。Phase 4 前补子任务：
1. 收集 spec 全部中文文案 → 去重得 ~100 字符子集
2. 用 `font2bitmap` 或脚本从 Noto Sans CJK 12px 抽取，生成 `mono_font` 兼容的 const 字节数组
3. 新增 `src/apps/view/font/cjk_subset.rs`，注册为 `MonoFont`
4. `Text::new("退出群组", ...).draw(fb)` 即可渲染中文

flash 占用 ~100 字符 × (12×12 bit / 8) ≈ 1.8KB/字 + 索引 ≈ 20-40KB，远低于 16MB flash 上限。

### 图标绘制

电量 4 档 / 信号 4 格 / 静音 / 模式（清晰=单人占用 / 自由=多人并发）全部用 embedded-graphics `Rectangle` / `Line` / `Polyline` 画。状态栏 240×20px：左侧模式图标 + 静音 + 组网，右侧信号 + 电量 + 时间。无位图资产，省 flash 且可缩放。

## Risks

- **RenderCtx 快照一致性**：快照与 service 真实值有 ms 级延迟。对状态栏（电量/信号/时间）可接受；对 IntercomApp 的 peer 卡片需确保 IntercomEvent 投递后 dirty 立即重绘。缓解：UiEvent 队列 drain 后强制 dirty。
- **CJK 子集字体生成**：`mono_font` 期望特定 bitmap 格式，自定义 CJK 子集需匹配其 `MonoFont` 结构。缓解：Phase 4 前先做 spike，验证 1 个汉字能渲染再扩量。
- **DMA 推送性能**：全屏 115KB 全量推送每次约 5-8ms @ 40MHz SPI。500ms tick 下 CPU 占用 <2%，可接受。若未来需 60fps 动画再引入局部脏矩形。
- **ArrayDeque crate 引入**：新增依赖。备选 `embassy_sync::channel::Channel`（已启用）避免新 crate。Phase 1 决定。
