## ADDED Requirements

### Requirement: 语音三态机
voice.rs SHALL 定义 `VoiceState` 枚举包含至少 `Idle`、`Talking`、`Listening`、`ChannelBusy` 四态，并实现状态迁移函数，迁移规则 SHALL 与 PRD §14.1 / 技术设计 §9.3 一致：Idle 在 PTT press 后迁移到 Talking（clear-mode 仲裁通过或 free-mode 直通）；Talking 在 PTT release 后迁移到 Idle；Idle 在收到他人 VOICE 包后迁移到 Listening；Listening 在收发结束后迁移到 Idle；clear-mode 仲裁失败（Condvar 等待窗口内收到他人 TALK_STATE(action=1)）迁移到 ChannelBusy，ChannelBusy 在 busy tone 播完后迁移回 Idle。

#### Scenario: Idle → Talking（free-mode）
- **WHEN** 系统处于 `Grouped(Idle)` 且模式为 Free，用户触发 PTT press
- **THEN** 状态迁移到 `Grouped(Talking)`，采集链启动、接收链关闭

#### Scenario: Idle → Talking（clear-mode 仲裁通过）
- **WHEN** 系统处于 `Grouped(Idle)` 且模式为 Clear，用户触发 PTT press，Condvar::wait_timeout(50ms) 窗口内未收到他人 TALK_STATE(action=1)（timeout）
- **THEN** 状态迁移到 `Grouped(Talking)`，采集链启动、接收链关闭

#### Scenario: Idle → ChannelBusy（clear-mode 仲裁失败）
- **WHEN** 系统处于 `Grouped(Idle)` 且模式为 Clear，用户触发 PTT press，Condvar 等待窗口内收到他人 TALK_STATE(action=1)（conflict_flag 被置 true 并唤醒等待方）
- **THEN** 状态迁移到 `Grouped(ChannelBusy)`，不进入 Talking，采集链保持关闭，播放忙态提示音

#### Scenario: Talking → Idle
- **WHEN** 系统处于 `Grouped(Talking)`，用户触发 PTT release
- **THEN** 状态迁移到 `Grouped(Idle)`，采集链关闭、接收链恢复待机

#### Scenario: Idle → Listening
- **WHEN** 系统处于 `Grouped(Idle)`，收到他人 VOICE 包
- **THEN** 状态迁移到 `Grouped(Listening)`，采集链关闭、接收链开启

### Requirement: 半双工互斥
voice.rs SHALL 在每次 VoiceState 迁移时调用 AudioService 接口保证半双工互斥规则：Idle 态采集链 OFF + 接收链 ON（待机）；Talking 态采集链 ON + 接收链 OFF；Listening 态采集链 OFF + 接收链 ON；ChannelBusy 态采集链 OFF + 接收链 ON（待机）。互斥规则 SHALL 与 PRD §14.2 一致。

#### Scenario: 进入 Talking 关闭接收链
- **WHEN** 状态迁移到 Talking
- **THEN** AudioService::stop_playback 被调用（或接收解码暂停），接收链 OFF；AudioService::start_capture 被调用，采集链 ON

#### Scenario: 进入 Listening 关闭采集链
- **WHEN** 状态迁移到 Listening
- **THEN** 采集链 OFF，接收链 ON

#### Scenario: 回到 Idle 恢复接收待机
- **WHEN** 状态从 Talking 或 Listening 迁移回 Idle
- **THEN** 采集链 OFF，接收链 ON（待机低功耗）

### Requirement: PTT press 流程
IntercomService::ptt_press SHALL 实现完整 PTT 按下流程：仅当当前状态为 `Grouped(Idle)` 时执行（其他状态直接返回错误或忽略）；clear-mode 下先发送 TALK_STATE(action=1) 包，然后以 `Condvar::wait_timeout(50ms)` 等待仲裁窗口，期间 ESP-NOW 接收回调（Task A recv 路径）解析 incoming TALK_STATE，若收到 action=1 则置 `conflict_flag` 并 `condvar.notify()` 提前唤醒；被唤醒后读 conflict_flag，若为冲突则迁移到 ChannelBusy 并播放忙态提示音、不进入 Talking；timeout 或无冲突则进 Talking。free-mode 下发送 TALK_STATE(action=1) 后不等仲裁直接进 Talking。进入 Talking 后依次执行 `audio.pa_enable(true)` → `audio.start_capture()`（预热）→ 播放就绪提示音 → 短保护间隔 → 启用 `capture_armed` 标志后 `on_capture_frame` 回调里 `encode_voice` 构造 VOICE 包并发送。流程 SHALL 与技术设计 §11/§19.4 一致。

#### Scenario: clear-mode 仲裁通过进入 Talking
- **WHEN** Grouped(Idle) + Clear 模式，ptt_press 被调用
- **THEN** 发送 TALK_STATE(action=1)，Condvar::wait_timeout(50ms) 等待仲裁，无冲突（timeout 或被唤醒且 conflict_flag=false）后状态=Talking，pa_enable(true)，start_capture，播放 ready tone，保护间隔后开始发送 VOICE 包

#### Scenario: free-mode 直接进入 Talking
- **WHEN** Grouped(Idle) + Free 模式，ptt_press 被调用
- **THEN** 发送 TALK_STATE(action=1)（供 UI），不等 50ms，直接状态=Talking，pa_enable(true)，start_capture，播放 ready tone，保护间隔后开始发送 VOICE 包

#### Scenario: 非 Idle 态调用 ptt_press 被忽略
- **WHEN** 当前状态为 Talking/Listening/ChannelBusy，ptt_press 被调用
- **THEN** 不改变状态，不重复启动采集，返回 Busy/InvalidState 错误或静默忽略

### Requirement: PTT release 流程
IntercomService::ptt_release SHALL 实现 PTT 松开流程，顺序遵循技术设计 §19.4：`capture_armed` 置 false → `audio.stop_capture()` → `audio.pa_enable(false)` → 若曾进入 Talking（state==Talking，clear/free-mode）则发送 TALK_STATE(action=0) → 状态迁移到 `Grouped(Idle)`。ChannelBusy 态 release SHALL NOT 发送 TALK_STATE(action=0)（因未占用频道）。

#### Scenario: Talking 态松开 BOOT
- **WHEN** 状态为 Talking，ptt_release 被调用
- **THEN** stop_capture，pa_enable(false)，发送 TALK_STATE(action=0)，状态=Idle

#### Scenario: ChannelBusy 态松开 BOOT
- **WHEN** 状态为 ChannelBusy，ptt_release 被调用
- **THEN** 状态迁移到 Idle（无采集需停止、无 TALK_STATE(action=0) 需发送，因从未进 Talking）

### Requirement: VOICE 包发送
Talking 态下 `on_capture_frame` 回调被触发时（且 `capture_armed=true`），voice.rs SHALL 使用 change 08 的 `encode_voice` 构造 VOICE 包（seq + sender_id + effect + opus_payload），并对组内每个非自身 peer 调用 `network.send_unicast(peer.mac, &pkt)` 发送。seq SHALL 单调递增。

#### Scenario: 采集帧触发 VOICE 包发送
- **WHEN** 状态为 Talking，capture_armed=true，on_capture_frame 回调收到一帧 AudioFrame
- **THEN** 构造 VOICE 包并向所有非自身 peer send_unicast

#### Scenario: 预热期帧不发送
- **WHEN** 状态为 Talking 但 capture_armed=false（ready tone + 保护间隔未结束）
- **THEN** on_capture_frame 回调收到的帧被丢弃，不构造 VOICE 包、不发送

### Requirement: BOOT 按键 PTT 去抖
InputService SHALL 对 BOOT 按下做 ≥50ms 去抖：按下持续时间 <50ms 发出 `BootShortTap` 事件（唤醒屏幕），≥50ms 发出 `BootPress` 事件（进入 PTT 流程）。`BootPress` 事件 SHALL 被路由到 IntercomService::ptt_press，`BootRelease` 事件 SHALL 被路由到 IntercomService::ptt_release。路由 SHALL 在已组网且对讲服务可用时生效，未组网时 SHALL NOT 触发 PTT。

#### Scenario: BOOT 长按触发 PTT
- **WHEN** 已组网，BOOT 按下持续 ≥50ms
- **THEN** 触发 BootPress 事件，IntercomService::ptt_press 被调用

#### Scenario: BOOT 短按唤醒屏幕
- **WHEN** BOOT 按下持续 <50ms
- **THEN** 触发 BootShortTap 事件，不调用 ptt_press，唤醒屏幕（若熄屏）

#### Scenario: 未组网时 BOOT 长按不触发 PTT
- **WHEN** 未组网（非 Grouped 态），BOOT 按下 ≥50ms
- **THEN** BootPress 事件被忽略，不调用 ptt_press

### Requirement: 全局跨 App PTT 路由
BOOT 长按作 PTT SHALL 在任意前台 App 中生效（不限于对讲 App），前提是已组网 + 对讲服务可用。路由 SHALL 在 Shell/InputService 层全局处理，而非各 App 自行处理。

#### Scenario: 在 Settings App 中按 BOOT 触发 PTT
- **WHEN** 已组网，前台 App 为 Settings，BOOT 长按 ≥50ms
- **THEN** 触发 ptt_press，进入 Talking

### Requirement: 熄屏 PTT 直通
熄屏状态下 BOOT 长按 SHALL 直接进入 PTT 流程，不触发亮屏。InputService SHALL 在熄屏状态下对 BOOT 长按仍发出 `BootPress { screen_was_off: true }` 事件（`screen_was_off` 字段定义于 change 04 的 `InputEvent::BootPress`，本变更不修改 InputEvent 枚举），IntercomService 收到后 SHALL NOT 调用 DisplayService::screen_on。熄屏短按仍 SHALL 仅唤醒屏幕。此行为与 change 15 ScreenPolicy 互补：change 15 在熄屏时转发 `BootGpioPress` 给 InputService 但不调 `screen_on()`，change 10 据此触发 PTT 不亮屏。

#### Scenario: 熄屏长按 PTT 不亮屏
- **WHEN** 屏幕熄灭，BOOT 按下 ≥50ms
- **THEN** 触发 ptt_press 进入 Talking，屏幕保持熄灭

#### Scenario: 熄屏收到他人语音不亮屏
- **WHEN** 屏幕熄灭，收到他人 VOICE 包
- **THEN** 状态迁移到 Listening，屏幕保持熄灭

### Requirement: 触摸 PTT 备用入口
对讲主页面 SHALL 提供大面积触摸 PTT 区域，touch-down 触发 IntercomService::ptt_press，touch-up 触发 IntercomService::ptt_release。触摸 PTT SHALL NOT 经过 50ms 去抖（按下即视为有意 PTT）。行为 SHALL 与 BOOT 长按一致。本约定在 intercom_app 中实现调用点，slint 页面布局在 change 13 落地。

#### Scenario: 触摸 PTT 按下
- **WHEN** 已组网，用户触摸对讲主页面的 PTT 区域
- **THEN** 调用 ptt_press，进入 Talking（与 BOOT 长按一致）

#### Scenario: 触摸 PTT 松开
- **WHEN** 用户手指离开 PTT 触摸区域
- **THEN** 调用 ptt_release，回到 Idle

### Requirement: clear-mode 近同时抢话退化
clear-mode 下近同时抢话（多设备几乎同时按下 PTT）时，若 50ms 仲裁窗口内因 ESP-NOW 传播延迟未检测到冲突，允许多设备同时进入 Talking（短时退化为近似自由模式效果）。恢复策略 SHALL 为：各设备在 PTT release 时发送 TALK_STATE(action=0)，下一轮 PTT 抢话时仲裁窗口重新生效，自然回到单人占用。此场景 SHALL 被视为已知可接受退化，非缺陷。

#### Scenario: 两设备近同时抢话都进 Talking
- **WHEN** A、B 几乎同时按下 PTT，50ms 内双方未收到对方 TALK_STATE(action=1)
- **THEN** A、B 都进入 Talking，允许短时共存

#### Scenario: 下一轮抢话恢复单人占用
- **WHEN** A、B 上一轮近同时抢话后都 release，C 在 clear-mode 下按下 PTT
- **THEN** 50ms 仲裁窗口重新生效，无冲突则 C 单人占用 Talking

### Requirement: TALK_STATE 包发送时机
voice.rs SHALL 在以下时机发送 TALK_STATE 包（type=0x03，由 change 08 `encode_talk_state` 构造）：clear-mode ptt_press 时发 action=1（用于仲裁）；free-mode ptt_press 时发 action=1（仅供 UI 展示，不仲裁）；clear-mode/free-mode ptt_release 时若曾进入 Talking（state==Talking）则发 action=0，发送顺序遵循技术设计 §19.4：stop_capture → pa_enable(false) → send TALK_STATE(action=0) → state=Idle。ChannelBusy 态 release SHALL NOT 发 action=0（因从未进 Talking，未占用频道）。

#### Scenario: clear-mode ptt_press 发 TALK_STATE(action=1)
- **WHEN** clear-mode ptt_press 执行
- **THEN** 发送 TALK_STATE 包，action=1

#### Scenario: free-mode ptt_press 发 TALK_STATE(action=1)
- **WHEN** free-mode ptt_press 执行
- **THEN** 发送 TALK_STATE 包，action=1

#### Scenario: Talking 态 release 发 TALK_STATE(action=0)
- **WHEN** Talking 态 ptt_release 执行
- **THEN** 发送 TALK_STATE 包，action=0

#### Scenario: ChannelBusy 态 release 不发 TALK_STATE
- **WHEN** ChannelBusy 态 ptt_release 执行
- **THEN** 不发送 TALK_STATE 包（因未占用频道）
