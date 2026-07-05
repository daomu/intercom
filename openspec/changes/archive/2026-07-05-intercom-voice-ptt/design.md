## Context

change 09 完成三阶段组队后，设备进入 `Grouped(VoiceState)` 状态，持有 peer 列表（mac + id + LMK）与加密 ESP-NOW 通道。change 05 完成后 AudioService 提供 start_capture/stop_capture/pa_enable/on_capture_frame/submit_pcm 接口。change 08 完成包格式：VOICE 包（type=0x01，含 seq/sender_id/effect/opus_payload）、TALK_STATE 包（type=0x03，action=1 开始 / 0 结束）。

当前缺口：voice.rs 尚未实现，VoiceState 仅是 trait 里的枚举占位；按下 BOOT 无任何响应；半双工互斥未落地；TALK_STATE 仲裁窗口未实现；VOICE 包发送链未打通。

本变更填上这块：把 voice.rs 落地为完整的语音三态机 + PTT 流程，让 2 台已组网设备能真正对讲。jitter/解码/混音/播放（接收链的下半段）留给 change 11，本变更只确保 Listening 态被正确进入、接收链处于 standby ON。

## Goals / Non-Goals

**Goals:**
- voice.rs 实现 VoiceState 四态（Idle / Talking / Listening / ChannelBusy）迁移，迁移规则与 PRD §14.1/§14.2 + 技术设计 §9.3 一致
- 半双工互斥表落地：Idle=采集OFF+接收ON standby；Talking=采集ON+接收OFF；Listening=采集OFF+接收ON
- PTT press 全流程：BOOT ≥50ms 去抖 → clear-mode 发 TALK_STATE(action=1) + 50ms 仲裁 → 冲突则 ChannelBusy+busy tone，无冲突则 Talking + pa_enable + start_capture 预热 + ready tone + on_capture_frame → `encode_voice` 构造 VOICE 包 → send_unicast 全 peer
- PTT release 全流程：stop_capture → pa_enable(false) → 发 TALK_STATE(action=0)（仅 clear/free-mode 曾进 Talking 时） → state=Idle
- free-mode 直通 Talking（无仲裁、不发 TALK_STATE 的仲裁用途；仍发 action=1 供 UI 展示——见 D3）
- 熄屏 PTT 直通（不亮屏）
- 任意前台 App 时 BOOT 长按作 PTT（全局输入路由）
- 触摸 PTT 备用入口（intercom_app 主页面大面积触摸区域）
- clear-mode 近同时抢话允许短时退化近似自由模式，尽快回单人占用

**Non-Goals:**
- jitter buffer / Opus 解码 / 多路混音 / PLC / 静音封底 → change 11
- 接收链的播放部分（submit_pcm 之后的混音输出） → change 11
- 变声处理（on_capture_frame 里的 effect 插入） → change 14
- 对讲主页面完整 UI 布局 → change 13（本变更仅约定触摸 PTT 入口存在与行为）
- 信道切换、心跳、冷启动恢复 → change 12
- TALK_STATE 包的 packet.rs 构造/解析函数 → change 08（本变更调用）

## Decisions

### D1：VoiceState 放在 voice.rs 而非 mod.rs
技术设计 §3.5 把 `VoiceState` 枚举声明在 IntercomService trait 文件里。本变更把状态机的**实现逻辑**（迁移函数、半双工互斥副作用、PTT 流程）放在 `src/intercom/voice.rs`，`mod.rs` 仅 `pub mod voice;` 并 `pub use voice::VoiceState;` 重导出。理由：mod.rs 后续会承载 IntercomService 的整体状态机（系统态/组队态/Grouped 态），voice.rs 专注 Grouped 内部三态，职责清晰。备选：全部塞 mod.rs——文件膨胀，排除。

### D2：50ms 仲裁窗口用 `Condvar::wait_timeout` 而非 `std::thread::sleep`
PTT press 发 TALK_STATE(action=1) 后需等 50ms 看是否收到他人 TALK_STATE。决策：**在 Task B（网络/业务，优先级 12）上用 `Condvar::wait_timeout(50ms)` 等待**，期间 ESP-NOW 收回调（Task A recv 路径）解析 incoming TALK_STATE，若收到 action=1 则置 `conflict_flag=true` 并 `condvar.notify()` 唤醒等待方；Task B 被唤醒后读 conflict_flag：true → ChannelBusy，false/timeout → 无冲突进 Talking。理由：相比 `std::thread::sleep`，Condvar 可在冲突到达时立即唤醒（不必等满 50ms），减少用户感知延迟；同时保持阻塞模型简单可读，不影响 Task A 音频实时性与 Task C UI。备选：非阻塞状态机 + 定时器回调——状态机碎片化，后续 change 11 接收链复杂度上升后维护成本高，排除。

### D3：free-mode 是否发 TALK_STATE？
PRD §13.1：clear-mode 用 TALK_STATE 仲裁频道占用；自由模式无冲突概念。但 TALK_STATE 包注释写"自由模式用于 UI 展示"。决策：**free-mode 也发 TALK_STATE(action=1/0)**，但不做仲裁（不等 50ms，直接进 Talking），仅供其他设备 UI 显示"X 正在发言"。clear-mode 的 TALK_STATE 才进入仲裁路径。备选：free-mode 完全不发——其他设备 UI 无法知道谁在发言，违反 PRD §13.1 的 UI 要求，排除。

### D4：50ms 去抖在 InputService 还是 IntercomService？
BOOT 按下 ≥50ms 才视为 PTT。决策：**在 InputService（Shell 层）做去抖**，<50ms 的短按发 `BootShortTap`（唤醒屏幕），≥50ms 的长按发 `BootPress`（进 PTT）。IntercomService 收到 `BootPress` 即直接进 ptt_press 流程，不再二次计时。理由：InputService 已有事件分类职责（技术设计 §3.6），且去抖逻辑与 PTT 业务解耦。备选：IntercomService 内部计时——跨层职责混乱，排除。

### D5：ready tone 与 capture 的时序
PRD §7.2：就绪提示音须在正式采集/发送前完成，加短保护间隔，不污染语音内容。但技术设计 §11/§19.4 伪代码写 `start_capture`（预热）→ `play_ready_tone`。决策：**先 `start_capture`（预热采集链，丢弃预热期帧）→ 播 ready tone → 短保护间隔（如 30ms）→ 之后的 capture frame 才正式 `encode_voice` 构造 VOICE 包并发送**。即 capture 启动早于"有效语音"开始，预热期帧丢弃不发送。理由：预热减少首字丢失（采集链启动有冷启动延迟），ready tone 与保护间隔叠加保证提示音不混入语音。备选：ready tone 后再 start_capture——首字丢失风险，排除。预热期丢帧策略：用一个 `capture_armed: bool` 标志，ready tone + 保护间隔结束后才置 true，`on_capture_frame` 回调里仅 armed=true 的帧才发送。

### D6：ChannelBusy 的自动恢复
clear-mode 下收到他人 TALK_STATE 导致 ChannelBusy，本机不进 Talking。决策：**ChannelBusy 是瞬态态，不持久化**——busy tone 播完后立即回 Idle（用户需松开 BOOT 再重新按）。不在 ChannelBusy 态持续监听他人 release。理由：简化状态机；用户感知为"按下没抢到，松开重试"，符合传统对讲机体验。备选：ChannelBusy 持续监听他人 release 后自动进 Talking——状态机复杂、用户等待感知差，排除。

### D7：触摸 PTT 入口的实现位置
对讲主页面（intercom_app）提供大面积触摸 PTT 区域。决策：**intercom_app 的 slint 页面定义一个 PTT 触摸区域，touch-down 调用 `IntercomService::ptt_press`，touch-up 调用 `ptt_release`**，行为与 BOOT 长按一致（但不经过 50ms 去抖——触摸按下即视为有意 PTT，无需去短按误触）。理由：触摸交互本身就是显式大面积区域，无短按误触问题；去抖仅针对物理按键。本变更仅约定行为契约与调用点，intercom_app 的 slint 页面实现在 change 13。备选：触摸也走 50ms 去抖——触摸无短按唤醒屏幕需求，无意义，排除。

### D8：熄屏 PTT 直通不亮屏
熄屏状态下 BOOT 长按直接进 PTT 流程，不触发 DisplayService::screen_on。决策：**InputService 在 screen_off 状态下收到 BOOT 长按（≥50ms）仍发 `BootPress { screen_was_off: true }`（`screen_was_off` 字段定义于 change 04 的 `InputEvent::BootPress`，本变更不修改 InputEvent 枚举）**；IntercomService 收到后正常进 PTT 流程，依据 `screen_was_off==true` 跳过 `screen_on()`。短按（<50ms）在熄屏下仍发 `BootShortTap` 唤醒屏幕。理由：PRD §7.2/§9.2 明确熄屏 PTT 不亮屏。备选：在 IntercomService 内查 is_screen_on——跨层查询 DisplayService，耦合，排除。

**与 change 15 的互补关系**：change 10 的 `BootPress { screen_was_off: true }` 触发 PTT 但不亮屏；change 15 的 ScreenPolicy 在熄屏时将 `BootGpioPress` 转发给 InputService 但不调用 `screen_on()`。两者互补不冲突——change 15 负责熄屏时把 BOOT 原始事件送到 InputService（不亮屏），change 04 的 InputService 据此生成 `BootPress { screen_was_off: true }`，change 10 据此进 PTT 跳过亮屏。详见 change 15 ScreenPolicy 设计。

### D9：clear-mode 近同时抢话退化策略
PRD §23/§13.1：极少数近同时抢话允许短时退化为近似自由模式，尽快回单人占用。决策：**50ms 仲裁窗口不是严格互斥锁**——若 A、B 几乎同时发 TALK_STATE(action=1)，可能双方都未在 50ms 内收到对方（ESP-NOW 传播延迟），导致双方都进 Talking。这是已知可接受退化。恢复策略：双方都在 release 时发 TALK_STATE(action=0)，下一轮 PTT 抢话时仲裁窗口重新生效，自然回到单人占用。不额外做冲突检测与强制退出。理由：PRD 明确"属已知可接受退化，非缺陷"；额外冲突检测增加复杂度且单射频下无法可靠检测。备选：Talking 态收到他人 TALK_STATE 即强制退回 Idle——破坏正在进行的发言体验，排除。

## Risks / Trade-offs

- **[50ms 仲裁窗口阻塞 Task B]** → Task B 在此期间 `Condvar::wait_timeout(50ms)`，冲突到达可提前唤醒；50ms 上限短暂，且 PTT 优先级高于心跳，可接受；若心跳超时风险，change 12 可调整心跳策略
- **[ESP-NOW 单播 4 peer 的发送时序]** → Talking 态每帧（20ms）需向最多 3 个 peer send_unicast，6 次加密发送可能挤占 Task A 时间；change 06 的 NetworkService 实现需保证 send_unicast 非阻塞入队；本变仅在 on_capture_frame 回调里调用，不阻塞采集
- **[预热期丢帧的 armed 标志竞态]** → on_capture_frame 回调在 Task A，armed 标志在 Task B 设置；用 `AtomicBool` 保证可见性；armed 置 true 前的帧直接丢弃
- **[ChannelBusy 态用户持续按住 BOOT]** → busy tone 播完回 Idle，但用户可能仍按住；本变更不自动重试，需用户松开重按；符合传统对讲机体验
- **[触摸 PTT 与 BOOT PTT 同时触发]** → 极罕见；以先到的 press 为准，后来的 press 在非 Idle 态直接返回 NotGrouped/Busy 错误（ptt_press 幂等性：非 Idle 态调用直接返回）
- **[free-mode 多人同时 Talking 的网络负载]** → N 人组最多 N 路 VOICE 包并发，单射频冲突概率上升；PRD 已明确自由模式"本机发言时不播放他人音频"且混音最多 2-3 路；网络退化由 change 11 的弱网退化优先级处理
- **[ready tone 与 capture 预热期的内存]** → 预热期丢弃的帧仍占采集缓冲；AudioService 的采集缓冲为预分配固定池（change 05 约束），无动态分配风险

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译
3. 烧录 2 台已组网设备（组关系由 change 09 建立）
4. A 按住 BOOT → 听到 ready tone → 讲话 → B 听到 A 语音（接收链 standby 已通，但解码/混音在 change 11 才完整；本期 B 侧仅状态迁移到 Listening，无实际播放）
5. A 松开 BOOT → B 收到 TALK_STATE(action=0)
6. clear-mode 下 B 在 A 发言时按 BOOT → 听到 busy tone，不进 Talking

回滚：`git revert <commit>`，voice.rs 移除，IntercomService 的 Grouped 态回退为占位。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
