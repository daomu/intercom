## Context

change 07 已落地 App trait（`on_enter`/`on_exit`/`on_event`/`on_tick` + `id`/`title`）、AppRegistry（注册/查找/枚举可见 App）、Launcher（前台切换、InputEvent 派发、全局顶部状态区渲染、熄屏策略、音量面板、静音 toggle）与 Settings App。change 10 已落地 `IntercomService` trait 全方法、`VoiceState`（Idle/Talking/Listening/ChannelBusy）、PTT press/release 流程（clear-mode 50ms 仲裁 / free-mode 直通）、`IntercomEvent` 订阅通道、触摸 PTT 备用入口契约、ChannelBusy 反馈契约（busy tone 已在 AudioService 实现）。

当前 `src/apps/intercom_app.rs` 与 `ui/intercom*.slint` 为空占位。本期在此壳上落地对讲 App 全部 UI 页面，不重复实现 PTT 状态机或音频路径（消费 change 10 的服务接口）。

硬件约束：ST7789 240×240 LCD，CST816 单点触摸（无多点），512KB SRAM 无 PSRAM。slint 渲染在 Task C（优先级 5 / 栈 12KB）。

## Goals / Non-Goals

**Goals:**
- 未组网页完整可用：创建主机（含模式选择 清晰/自由）→ 搜索主机列表（6 字段齐全）→ 加入流程页 → 成功进入已组网或失败回未组网
- 已组网三页面左右滑页可切换：主对讲页（自适应 1/2/3/4 卡片）/ 变声器页（档位+预览）/ 群组信息·退出页（二次确认）
- 顶部状态区与本机状态区实时反映 IntercomEvent 订阅的状态
- 主对讲页触摸 PTT 备用区域可触发 `ptt_press`/`ptt_release`（行为同 BOOT 长按）
- ChannelBusy 视觉反馈（状态区「频道忙」+ 成员卡片发言图标冻结）+ 复用 busy tone
- 全局静音开启时主页持续显示静音图标
- 清晰模式 vs 自由模式视觉差异可辨识
- 2 台已组网设备实际操作 UI 完成对讲流程无卡顿、无文字溢出

**Non-Goals:**
- PTT 状态机本身（由 change 10 实现，本期仅消费）
- 音频采集/播放/编解码（由 change 05/11 实现）
- 组队三阶段协议本身（由 change 09 实现，本期仅渲染状态与触发 `host_create`/`join_request` 等 API）
- 心跳/冷启动恢复（由 change 12 实现）
- 变声器 DSP 算法（由 change 14 实现，本期仅 UI 档位切换与预览入口调用 `preview_voice`）
- 熄屏策略本身（由 change 07/15 实现，本期不修改）
- 页面过渡动画与多语言（change 17 整合）

## Decisions

### D1：页面路由 = 状态驱动而非显式栈
Intercom App 内部根据 `IntercomService::state()` 渲染对应顶层页面：`Idle` → 未组网页；`Hosting`/`Joining` → 对应流程页（视为未组网页的子流程，仍属顶层未组网态）；`Grouped` → 三页面可滑页切换。备选：显式页面栈 + push/pop——对讲页层级浅（≤3），栈管理过度工程化，排除。

### D2：左右滑页 = slint `SwipeAction` / `TouchArea` 拖动手势
已组网三页面用 slint 的 `TouchArea` 监听水平拖动，松手时按位移阈值决定切换方向，配合 `animation` 偏移实现滑动过渡。备选：分页指示器点击切换——不符合 PRD §8.1「左右滑页展开」原则，排除。未组网页为单页流程不参与滑页。

### D3：自适应布局 = 成员数映射 + slint `if` 分支
主对讲页根据当前 peer 在线数（1/2/3/4）选择布局：1=单大卡（卡片占满主区域）/ 2=双卡（上下或左右）/ 3=三分布局（左大右两小）/ 4=四宫格。布局切换由 slint `if peer_count == 1 { ... } else if ...` 分支实现，成员数变化时即时重排。备选：动态 Grid——240×240 屏在 4 宫格下每格仅 120×120，固定布局更可控，排除动态 Grid。

### D4：成员卡片字段精简 = 严格遵循 PRD §8.3
每张卡片仅显示：名称 / 在线·离线图标 / 信号4格 / 发言图标。**不显示**：成员 ID 后四位（仅在搜索主机列表与组队确认页显示 MAC 后4位，主页不显示）、Host 标签、复杂文字状态列表。备选：在主页显示 Host 标签——违反 PRD §8.3 明确排除项，排除。

### D5：IntercomEvent → UI 刷新 = 单订阅回调 + slint `Model` 更新
Intercom App `on_enter` 时调用 `IntercomService::subscribe`，回调内根据 `IntercomEvent` 变体更新 slint `SharedString`/`Model`/`int`/`bool` 属性，slint 自动重渲染。回调线程与 slint 主循环线程间通过 `slint::invoke_from_event_loop` 安全投递。备选：轮询 `state()`——增加 CPU 占用且刷新滞后，排除。

### D6：触摸 PTT 备用区域 = 主对讲页底部大面积 `TouchArea`
主对讲页底部约 60px 高度区域为触摸 PTT 区，`pressed`→`IntercomService::ptt_press()`、`released`/`canceled`→`ptt_release()`，行为与 BOOT 长按一致（由 change 10 保证幂等与状态机仲裁）。备选：小按钮——不利于带手套/盲操场景，排除。

### D7：ChannelBusy 反馈 = 视觉优先 + 音效辅助
收到 `IntercomEvent::ChannelBusy` 时：本机状态区切到「频道忙」+ 主对讲页成员卡片发言图标冻结（不闪烁）+ 触摸 PTT 区视觉降权（灰色边框）+ 复用 change 10 的 busy tone 一次性播放。状态恢复（`PttReady` 或 `StateChanged`）后视觉解锁。备选：弹窗阻塞——打断收听体验，排除。

### D8：群组退出二次确认 = 模态覆盖层 + 文案精确
群组信息页「退出群组」按钮触发模态确认层，文案精确为「退出后本机将删除当前组信息」（PRD §21.3），二次点击「确认退出」才调用 `leave_group()`，否则「取消」关闭模态。备选：Toast 倒计时撤销——交互复杂且 PRD 明确要求二次确认，排除。

### D9：变声器页档位 = 三档 Normal/PitchUp/PitchDown
本期仅实现三档切换（与 `VoiceEffect` 枚举一致），点击档位调用 `set_voice_effect`，预览按钮调用 `preview_voice`（录 3s 本地播放，由 change 10/14 实现 DSP）。备选：滑块连续调节——DSP 不支持连续 pitch，排除。

### D10：模式视觉差异 = 图标 + 文案双通道
清晰模式：顶部状态区显示单人占用图标 + 本机状态提示「单人占用」；自由模式：显示多人并发图标 + 「可同时发言」。清晰模式下他人发言时本机进入 Listening（接收中），自由模式下多人发言时状态区仍为「接收中」但成员卡片多亮发言图标。备选：颜色区分——240×240 色彩信息冗余且色弱用户不友好，排除。

### D11：未组网页模式选择 = 创建主机时选清晰/自由
创建主机入口进入后，显示模式选择（清晰/自由）+ 当前成员列表（初始仅本机）+ 当前人数/上限 + 「确认组队」/「结束组队」按钮。模式选择在 `host_create` 调用前确定，传入 `IntercomMode`。备选：创建后改模式——破坏组队一致性，排除。

### D12：搜索主机列表项字段顺序 = 名称/MAC后4位/信号/人数/模式/可加入
按 PRD §11.5 顺序渲染，每行紧凑布局：名称（左对齐）+ MAC后4位（小字辅助标识）+ 信号4格图标 + 人数/上限 + 模式标签 + 可加入图标（✓/✗）。不可加入项点击无效或提示「人数已满/模式不兼容」。备选：省略可加入图标——用户需逐项点开判断，体验差，排除。

## Risks / Trade-offs

- **[slint 在 240×240 单点触摸下的滑页手势灵敏度]** → 阈值取水平位移 >40px 且垂直位移 <20px 判定为滑页；误触垂直滚动（本页无滚动）不影响；实测调优
- **[IntercomEvent 回调线程与 slint 主循环线程安全]** → 必须用 `slint::invoke_from_event_loop` 投递更新，禁止直接跨线程修改 slint 属性
- **[自适应布局 1→4 切换时卡片重排闪烁]** → slint 重渲染开销小（240×240），可接受；若明显则加 200ms 过渡动画（change 17 整合）
- **[IntercomEvent 高频 VoiceActive 导致 UI 抖动]** → VoiceActive 仅更新对应成员卡片发言图标，不触发整页重排；图标闪烁用 slint animation 节流
- **[触摸 PTT 区域与滑页手势冲突]** → 触摸 PTT 区在主对讲页底部固定区域，滑页手势在主内容区，垂直/水平位移判定位区分明
- **[静音图标与 Settings App 全局静音状态一致性]** → 静音图标状态来源为 `Settings.muted`（change 07 持久化），Settings App 修改后 Launcher 顶部状态区与本页同步读取
- **[SRAM 占用：5 个 slint 页面同时编译]** → slint 编译期生成代码体积增大但运行时仅当前页渲染；Flash 16MB 充足，SRAM 仅当前页组件实例化

## Migration Plan

无既有运行时 UI 需要迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证 slint 新文件编译通过
3. `cargo espflash flash --port <port> --release`
4. 两台设备组网后验证：未组网页流程 / 主对讲页自适应 / 变声器档位 / 群组退出二次确认 / 触摸 PTT / ChannelBusy 反馈

回滚：`git revert <commit>`，回到空占位 intercom_app.rs（已组网仍可后台对讲，但无 UI 可操作）。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
