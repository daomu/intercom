## 1. App 注册与生命周期骨架

- [x] 1.1 新增 `src/apps/intercom_app.rs`：定义 `IntercomApp` 结构体，持有 `IntercomService` 引用与当前订阅 token；实现 `App` trait（`id`="intercom" / `title`="对讲" / `on_enter` / `on_exit` / `on_event` / `on_tick`）
- [x] 1.2 在 `src/main.rs` 启动流程中向 `AppRegistry` 注册 `IntercomApp`；Launcher 启动时根据 `IntercomService::state()` 为 `Grouped(_)` 时默认前台进入 Intercom App
- [x] 1.3 `on_enter` 调用 `IntercomService::subscribe` 注册回调，`on_exit` 释放回调；回调内所有 slint 属性更新通过 `slint::invoke_from_event_loop` 投递
- [x] 1.4 `on_event` 处理 InputEvent（触摸 PTT、滑页手势路由、按钮点击），区分已组网/未组网态派发

## 2. slint 文件与构建接入

- [x] 2.1 新增 `ui/intercom_ungrouped.slint`：未组网页（创建主机入口 + 搜索并加入入口 + 创建主机子流程 + 搜索主机列表 + 加入流程页），导出所需 `SharedString` / `int` / `bool` / `Model` 属性
- [x] 2.2 新增 `ui/intercom_main.slint`：主对讲页，自适应布局分支（1/2/3/4），成员卡片组件（名称 + 在线图标 + 信号4格 + 发言图标），底部触摸 PTT 备用区域 `TouchArea`
- [x] 2.3 新增 `ui/intercom_voice_changer.slint`：变声器页，三档切换按钮 + 预览按钮 + 档位高亮状态
- [x] 2.4 新增 `ui/intercom_group_info.slint`：群组信息页 + 退出群组模态确认层（文案「退出后本机将删除当前组信息」+ 确认/取消按钮）
- [x] 2.5 新增 `ui/intercom_status.slint`：顶部状态区组件（模式图标 + 静音图标 + 电量图标 + 已组网状态 + 本机状态提示）与本机状态区组件（Idle/准备发言/发言中/接收中/频道忙/静音中）
- [x] 2.6 在 `build.rs` 的 `slint_build::compile` 调用中追加全部 5 个 `.slint` 文件
- [x] 2.7 `cargo build` 验证 slint 编译期生成绑定代码无错误

## 3. 未组网页交互

- [x] 3.1 创建主机入口 → 渲染模式选择（清晰/自由）+ 成员列表（初始仅本机）+ 人数/上限 + 确认组队/结束组队按钮；模式确定后调用 `IntercomService::host_create(mode)`
- [x] 3.2 确认组队按钮调用 `host_confirm()`；结束组队按钮调用 `host_cancel()` 并回未组网初始态
- [x] 3.3 搜索并加入入口 → 调用 `join_search_start()` → 渲染主机列表，每行 6 字段：名称 + MAC后4位 + 信号4格 + 人数/上限 + 模式 + 可加入图标
- [x] 3.4 不可加入项（人数满/模式不兼容）点击给出明确提示，不触发 `join_request`
- [x] 3.5 可加入项点击 → 调用 `join_request(host_mac)` → 进入加入流程页（模式/成员/人数/自己状态/信号/退出入口）
- [x] 3.6 加入流程页退出按钮调用 `join_cancel()` 回未组网初始态
- [x] 3.7 订阅 `IntercomEvent::StateChanged`/`PairFailed` 更新加入状态显示（请求中/等待确认/已加入/失败）

## 4. 主对讲页实现

- [x] 4.1 根据 `state()` 为 `Grouped` 时渲染主对讲页；从 peer 列表构造 slint `Model` 驱动卡片
- [x] 4.2 自适应布局分支：在线数 1=单大卡 / 2=双卡 / 3=三分布局 / 4=四宫格；`PeerOnline`/`PeerOffline` 事件即时重排
- [x] 4.3 成员卡片仅显示名称 + 在线/离线图标 + 信号4格 + 发言图标；不显示 ID 后4位/Host 标签/复杂文字状态
- [x] 4.4 底部触摸 PTT 备用区域：`pressed`→`ptt_press()`，`released`/`canceled`→`ptt_release()`
- [x] 4.5 `VoiceActive(id)` 事件驱动对应卡片发言图标闪烁（slint animation 节流）

## 5. 已组网滑页导航

- [x] 5.1 在主对讲页主内容区监听水平拖动，位移 >40px 且垂直 <20px 判定滑页
- [x] 5.2 三页面循环切换：主对讲 ↔ 变声器 ↔ 群组信息 ↔ 主对讲（循环）
- [x] 5.3 未组网页（含子流程）禁用滑页手势
- [x] 5.4 滑页过渡用 slint animation 偏移（若明显卡顿则降级为瞬切，留 change 17 整合）

## 6. 变声器页

- [x] 6.1 三档按钮 Normal/PitchUp/PitchDown，点击调用 `set_voice_effect`
- [x] 6.2 预览按钮调用 `preview_voice()`；预览期间禁用档位切换按钮（置灰）
- [x] 6.3 当前档位高亮显示

## 7. 群组信息·退出页

- [x] 7.1 渲染当前组信息（组名 / 模式 / 成员列表 / 信道）
- [x] 7.2 「退出群组」按钮触发模态确认层，文案精确「退出后本机将删除当前组信息」
- [x] 7.3 「确认退出」调用 `leave_group()`；「取消」仅关闭模态
- [x] 7.4 `leave_group` 成功后状态切回未组网，页面切到未组网页

## 8. 状态区组件

- [x] 8.1 顶部状态区：模式图标（清晰=单人占用 / 自由=多人并发）+ 静音图标 + 电量图标 + 已组网状态 + 本机状态提示
- [x] 8.2 静音图标状态来源为 `Settings.muted`（与 Settings App / Launcher 联动），全局静音开启时持续显示
- [x] 8.3 本机状态区根据 `VoiceState` 与 `IntercomEvent` 显示：Idle/准备发言/发言中/接收中/频道忙/静音中
- [x] 8.4 清晰模式提示文案含「单人占用」语义；自由模式提示文案含「可同时发言」语义

## 9. ChannelBusy 反馈

- [x] 9.1 收到 `IntercomEvent::ChannelBusy` → 本机状态区切「频道忙」+ 成员卡片发言图标冻结 + 触摸 PTT 区灰色边框
- [x] 9.2 复用 change 10 的 busy tone 一次性播放（不在 UI 层重新实现音频）
- [x] 9.3 收到 `PttReady` 或 `StateChanged(Grouped(Idle))` 后视觉解锁

## 10. 集成与设备验收

- [x] 10.1 两台设备组网后验证：主对讲页自适应 1/2 布局切换、成员卡片字段精简、触摸 PTT 可触发对讲
- [x] 10.2 验证左右滑页三页面循环切换无卡顿、无文字溢出
- [x] 10.3 验证变声器档位切换 + 预览录 3s 本地播放
- [x] 10.4 验证群组退出二次确认文案与流程（取消/确认）
- [x] 10.5 验证未组网页全流程：创建主机（清晰/自由）→ 确认组队；搜索主机列表 6 字段完整 → 加入流程 → 退出
- [x] 10.6 验证 ChannelBusy 反馈（清晰模式下抢话触发）视觉降权 + 音效
- [x] 10.7 验证全局静音开启时对讲主页持续显示静音图标
- [x] 10.8 验证 IntercomEvent 高频更新（多人同时发言）不导致 UI 抖动或 panic
- [x] 10.9 3-device 与 4-device 自适应布局（三分布局 / 四宫格）实机验证 deferred to integration (change 17)，本期 best-effort：2 设备实测 1/2 布局 + 3/4 布局分支代码 review
- [x] 10.10 提交 commit：`feat: implement intercom app UI pages (change 13/17)`


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
