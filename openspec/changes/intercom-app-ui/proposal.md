## Why

17 个变更的第 13 个。change 07 已落地 App trait + 注册表 + Launcher 调度壳与 Settings App，change 10 已落地 VoiceState（Idle/Talking/Listening/ChannelBusy）与 PTT 全流程并暴露 IntercomEvent 订阅。但 `src/apps/intercom_app.rs` 与 `ui/intercom*.slint` 仍是占位空文件，用户组网/未组网后无可操作的页面——既看不到主机列表、加不进组，也无法在已组网页看到成员状态、切换变声档位、退出群组。需要在本期把对讲 App 的全部 slint UI 页面落地，覆盖未组网页（创建主机 / 搜索加入 / 加入过程）、主对讲页（自适应成员卡片）、变声器页、群组信息·退出页、顶部状态区与本机状态区，并与 IntercomService 的 IntercomEvent 订阅、触摸 PTT 备用入口、ChannelBusy 视觉反馈对接，使 2 台已组网设备具备完整可用的对讲 UI 体验。

## What Changes

- 新增 `src/apps/intercom_app.rs`：实现 `App` trait，注册到 `AppRegistry`（id="intercom", title="对讲"），生命周期 `on_enter`/`on_exit`/`on_event`/`on_tick` 接入 InputEvent 与 IntercomEvent 订阅回调
- 新增 `ui/intercom_ungrouped.slint`：未组网页，含「创建主机入口」按钮 + 「搜索并加入」按钮；创建主机流程显示当前模式（清晰/自由）选择、当前成员列表、当前人数/上限、确认组队/结束组队按钮；搜索主机列表每行字段：主机名称 + MAC后4位 + 信号4格 + 当前人数/上限 + 当前模式 + 是否可加入；加入流程页显示当前群组模式、当前成员列表、当前人数、自己加入状态、当前信号、退出本次流程入口
- 新增 `ui/intercom_main.slint`：主对讲页，自适应成员卡片布局（1=单大卡 / 2=双卡 / 3=三分布局 / 4=四宫格），每张卡片显示名称 + 在线/离线图标 + 信号4格 + 发言图标；**不显示**成员 ID 后四位、Host 标签、复杂文字状态列表；主页面含大面积触摸 PTT 备用区域（行为同 BOOT 长按 PTT）
- 新增 `ui/intercom_voice_changer.slint`：变声器页，档位切换（Normal / PitchUp / PitchDown）+ 预览入口（调用 `IntercomService::preview_voice`）
- 新增 `ui/intercom_group_info.slint`：群组信息·退出页，查看当前组信息（组名、模式、成员、信道）+ 退出群组按钮，二次确认文案明确「退出后本机将删除当前组信息」
- 新增 `ui/intercom_status.slint`：顶部状态区组件（模式图标 + 静音图标 + 电量图标 + 已组网状态 + 本机状态提示）与本机状态区组件（Idle / 准备发言 / 发言中 / 接收中 / 频道忙 / 静音中）
- 实现左右滑页导航：已组网状态下三个页面（主对讲页 / 变声器页 / 群组信息页）支持触摸左右滑切换；未组网页面不支持滑页（单页流程）
- 实现清晰模式 vs 自由模式视觉差异：模式图标区分（清晰=单人占用图标 / 自由=多人并发图标），状态提示文案差异化
- 实现 ChannelBusy 反馈：视觉优先（状态区「频道忙」+ 成员卡片发言图标冻结）+ 音效辅助（复用 change 10 的 busy tone）
- 实现全局静音图标持久显示：静音开启时对讲主页持续显示静音图标（与 Settings App 全局静音状态联动）
- `build.rs` 的 `slint_build::compile` 增加新 `.slint` 文件
- `main.rs` 启动时向 `AppRegistry` 注册 Intercom App（id="intercom"），Launcher 启动时若已组网则默认进入 Intercom App 前台

## Capabilities

### New Capabilities
- `intercom-app-ui`: 对讲 App 的全部 slint UI 页面（未组网页 / 主对讲页 / 变声器页 / 群组信息·退出页 / 状态区组件）与交互逻辑（左右滑页、自适应布局、触摸 PTT 备用入口、ChannelBusy 视觉反馈、IntercomEvent 订阅驱动状态刷新）

### Modified Capabilities
<!-- 无既有 spec 行为变更；App trait 与注册表行为由 change 07 spec 定义，本期仅消费 -->

## Impact

- **代码**：实质填充 `src/apps/intercom_app.rs`；新增 `ui/intercom_ungrouped.slint`、`ui/intercom_main.slint`、`ui/intercom_voice_changer.slint`、`ui/intercom_group_info.slint`、`ui/intercom_status.slint`
- **构建**：`build.rs` 的 `slint_build::compile` 列表新增上述 `.slint` 文件
- **依赖**：无新 crate（复用 change 01 声明的 `slint` / `esp-idf-svc` / `log` / `anyhow`）；变更依赖 = change 07（App trait / AppRegistry / Launcher）、08（IntercomState / packet format）、10（PTT / VoiceState / IntercomEvent）、12（heartbeat / restore state）、14（preview_voice DSP）
- **服务消费**：调用 change 07 的 `App` trait + `AppRegistry` + `Launcher`（注册、前台切换、InputEvent 派发、顶部状态区渲染钩子）；调用 change 09 的 `IntercomService` 配对方法（`host_create` / `host_confirm` / `host_cancel` / `join_search_start` / `join_request` / `join_cancel` / `leave_group`）；调用 change 10 的 `IntercomService` 语音方法（`state` / `ptt_press` / `ptt_release` / `subscribe`）；调用 change 14 的变声器方法（`set_voice_effect` / `preview_voice`）；订阅 `IntercomEvent`（StateChanged / PeerOnline / PeerOffline / VoiceActive / ChannelBusy / PttReady / PairFailed）驱动 UI 刷新
- **后续变更**：change 14（voice-changer）在本变更的变声器页档位切换上接续扩展更多档位与预览增强；change 15（power-screen-management）复用本变更的状态区组件；change 17（integration-polish）整合页面过渡动画与文案微调
- **PRD 对齐**：覆盖 PRD §8（显示/状态栏/页面层级：§8.1 低信息密度原则 / §8.2 页面结构 / §8.3 自适应卡片 / §8.4 状态区）、§11.5（搜索主机列表字段）、§21（对讲 App 页面需求）、§23（全局静音图标持久）；技术设计 §15（显示与页面层级）、§1.2（Intercom App 在 Applications 层）
