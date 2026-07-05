## ADDED Requirements

### Requirement: 对讲 App 注册与生命周期
`src/apps/intercom_app.rs` SHALL 实现 change 07 定义的 `App` trait，`id` 返回 `"intercom"`、`title` 返回 `"对讲"`；SHALL 在 `main.rs` 启动时通过 `AppRegistry::register` 注册；Launcher 启动时若 `IntercomService::state()` 返回 `Grouped(_)` SHALL 默认将 Intercom App 置为前台。

#### Scenario: 已组网设备上电默认进入对讲页
- **WHEN** 设备冷启动恢复后 `IntercomService::state()` 为 `Grouped(Idle)`
- **THEN** Launcher 启动后前台 App 为 Intercom App，主对讲页渲染成员卡片

#### Scenario: 未组网设备从 Launcher 进入对讲
- **WHEN** 设备未组网且用户在 Launcher 点击对讲图标
- **THEN** Intercom App 进入前台，渲染未组网页（创建主机 / 搜索并加入）

#### Scenario: App 生命周期回调被调用
- **WHEN** Launcher 切换前台 App 为 Intercom
- **THEN** `on_enter` 被调用并订阅 `IntercomEvent`；切换走时 `on_exit` 被调用并取消订阅

### Requirement: 未组网页——创建主机流程
未组网页 SHALL 提供「创建主机」入口；进入后 SHALL 显示模式选择（清晰 / 自由）、当前成员列表、当前人数 / 上限（MAX_GROUP_SIZE）、「确认组队」与「结束组队」按钮；模式选择 SHALL 在调用 `host_create` 前确定并传入对应 `IntercomMode`；「确认组队」SHALL 调用 `IntercomService::host_confirm`；「结束组队」SHALL 调用 `host_cancel` 并回到未组网初始态。

#### Scenario: 选择清晰模式创建主机
- **WHEN** 用户在创建主机页选择「清晰」并点击「开始创建」
- **THEN** `IntercomService::host_create(IntercomMode::Clear)` 被调用，页面显示当前成员列表与人数 1/4

#### Scenario: 确认组队冻结
- **WHEN** Host 在 CollectingPeers 阶段点击「确认组队」
- **THEN** `host_confirm()` 被调用，页面进入冻结等待态，不再接受新成员加入

#### Scenario: 结束组队回未组网
- **WHEN** Host 点击「结束组队」
- **THEN** `host_cancel()` 被调用，页面回到未组网页初始态（创建主机 / 搜索并加入）

### Requirement: 未组网页——搜索主机列表字段
搜索主机列表每行 SHALL 完整显示 6 个字段，顺序为：主机名称 / MAC后4位（轻量辅助标识）/ 信号4格 / 当前人数·上限 / 当前模式（清晰/自由）/ 是否可加入；字段 SHALL 不省略；列表项 SHALL 区分可加入与不可加入状态（不可加入项点击无效或给出明确提示）。

#### Scenario: 列表字段完整显示
- **WHEN** 搜索主机列表渲染一项
- **THEN** 该行同时显示：主机名称、MAC后4位（小字辅助）、信号4格图标、`当前人数/上限`、模式标签、可加入图标（✓ 或 ✗）

#### Scenario: 不可加入项点击无效
- **WHEN** 用户点击「人数已满」或「模式不兼容」的列表项
- **THEN** 不触发 `join_request`，给出明确提示（如「人数已满」），不阻塞其他项操作

#### Scenario: 主页面不显示辅助标识
- **WHEN** 设备进入已组网，主对讲页渲染成员卡片
- **THEN** 卡片不显示 MAC 后4位（辅助标识仅存在于搜索主机列表与组队确认页）

### Requirement: 未组网页——加入流程页
用户选择可加入主机并点击后 SHALL 进入加入流程页，显示：当前群组模式、当前成员列表、当前人数、自己加入状态（请求中/等待确认/已加入/失败）、当前信号、退出本次流程入口；点击退出 SHALL 调用 `join_cancel` 回未组网初始态。

#### Scenario: 加入请求中显示状态
- **WHEN** 用户点击可加入主机后 `join_request` 被调用
- **THEN** 加入流程页显示「请求中」状态，并显示目标主机模式、成员列表、人数、当前信号

#### Scenario: 退出加入流程
- **WHEN** 用户在加入流程页点击「退出」
- **THEN** `join_cancel()` 被调用，页面回到未组网页初始态

### Requirement: 主对讲页自适应成员卡片布局
主对讲页 SHALL 根据当前在线 peer 数量自适应布局：1=单大卡、2=双卡、3=三分布局、4=四宫格；每张成员卡片 SHALL 仅显示：名称、在线/离线图标、信号4格、发言图标；SHALL NOT 显示成员 ID 后四位、Host 标签、复杂文字状态列表；成员数变化时 SHALL 即时重排。

#### Scenario: 单成员单大卡
- **WHEN** 已组网且在线 peer 数为 1
- **THEN** 主对讲页渲染单张占满主区域的成员卡片

#### Scenario: 四成员四宫格
- **WHEN** 在线 peer 数为 4
- **THEN** 主对讲页渲染 2×2 四宫格，每格约 120×120

#### Scenario: 成员离线即时重排
- **WHEN** 收到 `IntercomEvent::PeerOffline` 导致在线数从 3 降为 2
- **THEN** 布局即时从三分布局切换为双卡，不残留离线卡片占位

### Requirement: 主对讲页触摸 PTT 备用区域
主对讲页 SHALL 在底部提供大面积触摸 PTT 区域；`pressed` SHALL 调用 `IntercomService::ptt_press()`；`released` 或 `canceled` SHALL 调用 `ptt_release()`；行为 SHALL 与 BOOT 长按 PTT 一致（由 change 10 保证幂等与状态机仲裁）。

#### Scenario: 触摸 PTT 触发发言
- **WHEN** 已组网 Grouped(Idle) 状态下用户按下触摸 PTT 区域
- **THEN** `ptt_press()` 被调用，状态区切到「准备发言」或「发言中」（由 clear-mode 仲裁结果决定）

#### Scenario: 触摸 PTT 与 BOOT 互不冲突
- **WHEN** 用户同时触发触摸 PTT 与 BOOT 长按
- **THEN** change 10 的状态机保证仅一次 `ptt_press` 生效，无重复发送

### Requirement: 已组网三页面左右滑页导航
已组网页面 SHALL 支持三页面左右滑切换：页面1=主对讲页、页面2=变声器页、页面3=群组信息·退出页；水平拖动位移 >40px 且垂直位移 <20px 时判定为滑页；未组网页（含子流程页）SHALL NOT 支持滑页。

#### Scenario: 主对讲页右滑进变声器页
- **WHEN** 用户在主对讲页向右拖动并松手，位移 >40px
- **THEN** 页面切换到变声器页

#### Scenario: 滑页手势不误触发垂直操作
- **WHEN** 用户在主对讲页垂直拖动成员卡片
- **THEN** 不触发滑页（垂直位移 ≥20px）

### Requirement: 变声器页档位与预览
变声器页 SHALL 提供三档切换：Normal / PitchUp / PitchDown；点击档位 SHALL 调用 `IntercomService::set_voice_effect`；SHALL 提供预览入口，点击调用 `IntercomService::preview_voice`（录 3s 本地播放）；预览期间 SHALL 禁用档位切换按钮。

#### Scenario: 切换档位
- **WHEN** 用户点击 PitchUp 档位
- **THEN** `set_voice_effect(VoiceEffect::PitchUp)` 被调用，UI 高亮当前档位

#### Scenario: 预览期间禁用切换
- **WHEN** 预览进行中（preview_voice 未返回）
- **THEN** 档位切换按钮置灰不可点

### Requirement: 群组信息·退出页二次确认
群组信息页 SHALL 显示当前组信息（组名、模式、成员、信道）；「退出群组」按钮 SHALL 触发模态确认层，文案精确为「退出后本机将删除当前组信息」；用户点击「确认退出」SHALL 调用 `IntercomService::leave_group()`；点击「取消」SHALL 仅关闭模态不调用任何服务。

#### Scenario: 二次确认文案精确
- **WHEN** 用户点击「退出群组」
- **THEN** 模态层显示文案「退出后本机将删除当前组信息」，并提供「确认退出」与「取消」两个按钮

#### Scenario: 取消退出
- **WHEN** 用户在二次确认模态点击「取消」
- **THEN** 模态关闭，组状态不变，`leave_group` 不被调用

#### Scenario: 确认退出
- **WHEN** 用户在二次确认模态点击「确认退出」
- **THEN** `leave_group()` 被调用，状态切回未组网，页面切到未组网页

### Requirement: 顶部状态区内容
顶部状态区 SHALL 显示：模式图标（清晰/自由）、静音图标、电量图标、已组网状态、本机状态提示；静音图标 SHALL 在全局静音开启时持续显示（与 Settings App 全局静音状态联动）；状态来源 SHALL 为 `IntercomService::state()` 与 `IntercomEvent` 订阅。

#### Scenario: 静音图标持久显示
- **WHEN** 全局静音开启且用户在对讲主页
- **THEN** 顶部状态区持续显示静音图标，不因页面切换或状态变化消失

#### Scenario: 模式图标区分
- **WHEN** 已组网模式为 Clear
- **THEN** 顶部状态区显示清晰模式图标；模式为 Free 时显示自由模式图标

### Requirement: 本机状态区状态值
本机状态区 SHALL 根据 `VoiceState` 与 `IntercomEvent` 显示以下状态之一：Idle（待机）/ 准备发言 / 发言中 / 接收中 / 频道忙 / 静音中；状态切换 SHALL 由 `IntercomEvent` 订阅驱动即时刷新。

#### Scenario: 频道忙状态显示
- **WHEN** 收到 `IntercomEvent::ChannelBusy`
- **THEN** 本机状态区显示「频道忙」

#### Scenario: 发言中状态显示
- **WHEN** `VoiceState` 切到 Talking
- **THEN** 本机状态区显示「发言中」

### Requirement: ChannelBusy 视觉反馈
收到 `IntercomEvent::ChannelBusy` 时 SHALL 提供视觉优先反馈：本机状态区切到「频道忙」、主对讲页成员卡片发言图标冻结、触摸 PTT 区视觉降权（灰色边框）；SHALL 辅以一次性音效（复用 change 10 的 busy tone）；状态恢复（`PttReady` 或 `StateChanged`）后视觉 SHALL 解锁。

#### Scenario: ChannelBusy 视觉降权
- **WHEN** 收到 ChannelBusy 事件
- **THEN** 触摸 PTT 区域显示灰色边框，成员卡片发言图标停止闪烁

#### Scenario: 状态恢复后解锁
- **WHEN** ChannelBusy 后收到 `PttReady` 或 `StateChanged(Grouped(Idle))`
- **THEN** 触摸 PTT 区域恢复常态，成员卡片发言图标恢复正常行为

### Requirement: 清晰模式与自由模式视觉差异
清晰模式与自由模式 SHALL 在顶部状态区图标与本机状态提示文案上区分：清晰模式显示单人占用图标 + 「单人占用」提示；自由模式显示多人并发图标 + 「可同时发言」提示。

#### Scenario: 清晰模式视觉
- **WHEN** 已组网模式为 Clear 且本机 Idle
- **THEN** 顶部状态区显示清晰模式图标，本机状态提示包含「单人占用」语义

#### Scenario: 自由模式视觉
- **WHEN** 已组网模式为 Free 且本机 Idle
- **THEN** 顶部状态区显示自由模式图标，本机状态提示包含「可同时发言」语义

### Requirement: IntercomEvent 订阅驱动 UI 刷新
Intercom App `on_enter` SHALL 调用 `IntercomService::subscribe` 注册回调；回调内 SHALL 根据事件变体更新 slint 属性；跨线程更新 SHALL 通过 `slint::invoke_from_event_loop` 安全投递；`on_exit` SHALL 取消订阅（或释放回调）。

#### Scenario: PeerOnline 即时刷新卡片
- **WHEN** 收到 `IntercomEvent::PeerOnline(id, rssi_4)`
- **THEN** 对应成员卡片在线图标亮起、信号4格更新为 rssi_4

#### Scenario: 跨线程安全投递
- **WHEN** IntercomEvent 回调在非 slint 主循环线程触发
- **THEN** slint 属性更新通过 `invoke_from_event_loop` 投递，不发生数据竞争或 panic

### Requirement: slint 文件接入构建
`build.rs` 的 `slint_build::compile` SHALL 包含全部新增 `.slint` 文件（`ui/intercom_ungrouped.slint` / `ui/intercom_main.slint` / `ui/intercom_voice_changer.slint` / `ui/intercom_group_info.slint` / `ui/intercom_status.slint`）；`.slint` 语法错误时构建 SHALL 失败并输出错误位置。

#### Scenario: slint 语法错误阻断构建
- **WHEN** `ui/intercom_main.slint` 存在语法错误
- **THEN** `cargo build` 失败，`slint_build` 输出错误文件与行号

#### Scenario: slint 文件变更触发重编译
- **WHEN** 修改任一 `ui/intercom_*.slint` 后执行 `cargo build`
- **THEN** slint 绑定代码重新生成，主 crate 重新编译
