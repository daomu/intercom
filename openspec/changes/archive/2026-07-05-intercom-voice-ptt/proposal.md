## Why

17 个变更提案的第 10 个。change 09 完成后设备已能组网并持有 peer 列表与加密 ESP-NOW 通道，change 05 完成后 AudioService 已具备采集/播放/PA 控制能力，但 Grouped 状态下用户按下 BOOT 仍无任何语音路径打通——既无半双工互斥、也无 TALK_STATE 仲裁、更无 VOICE 包发送。需要把 `src/intercom/voice.rs` 落地为完整的语音三态机与 PTT 流程，让 2 台已组网设备能真正对讲起来，并为 change 11（jitter/mixing 接收链）提供 Talking 态发送侧的输入源。

## What Changes

- 新增 `src/intercom/voice.rs`：实现 `VoiceState` 枚举（Idle / Talking / Listening / ChannelBusy）与状态机迁移逻辑，由 IntercomService 在 `Grouped(VoiceState)` 状态下驱动
- 实现 PTT press 流程：BOOT 按下 ≥50ms 阈值去抖 → clear-mode 先发 TALK_STATE(action=1) → 等 50ms 仲裁窗口（期间收到他人 TALK_STATE 即 ChannelBusy + busy tone + 不进 Talking）/ free-mode 直接进 Talking
- 实现 Talking 态半双工互斥：采集链 ON + 接收链 OFF；`audio.pa_enable(true)` → `audio.start_capture()` 预热 → 就绪提示音（在正式采集前完成 + 保护间隔，避免污染语音首字）→ `on_capture_frame` 回调里 `encode_voice` 构造 VOICE 包 → `network.send_unicast` 到所有组内 peer（except self）
- 实现 PTT release 流程：`audio.stop_capture()` → `audio.pa_enable(false)` → 发 TALK_STATE(action=0)（仅 clear/free-mode 曾进 Talking 时）→ state=Idle
- 实现 Listening 态半双工互斥：采集链 OFF + 接收链 ON（待机）；收到他人 VOICE 包触发 Listening（本变更仅完成状态迁移与接收链 standby，jitter/解码/混音在 change 11）
- 实现 Idle 态：采集 OFF + 接收 ON standby
- 实现 BOOT 按键 PTT 路由：InputService 的 `BootPress { screen_was_off }`/`BootRelease` 事件（`BootPress` 字段定义于 change 04）路由到 IntercomService::ptt_press/ptt_release，仅在 `Grouped` 且服务可用时生效；短按 <50ms 视为唤醒屏幕，不作 PTT
- 实现熄屏 PTT 直通：熄屏状态下 BOOT 长按直接进 PTT 流程，不触发亮屏；`BootPress { screen_was_off: true }` 携带熄屏标志，IntercomService 据此跳过 `screen_on()`（与 change 15 ScreenPolicy 互补：change 15 转发 `BootGpioPress` 给 InputService 但不调 `screen_on()`）
- 实现任意前台 App 时的 PTT：已组网 + 对讲服务可用时，BOOT 长按在任何前台 App 中都作 PTT（全局输入路由，非 intercom_app 专属）
- 实现触摸 PTT 备用入口：对讲主页面提供大面积触摸 PTT 区域，行为与 BOOT 长按一致
- 实现 clear-mode 近同时抢话退化：允许短时近似自由模式效果，尽快回单人占用（不视作缺陷）
- 复用 change 08 的 `encode_voice`（seq + sender_id + effect + opus_payload）与 `encode_talk_state`/`parse_talk_state` 包构造/解析
- 复用 change 09 的 Grouped 状态、peer 列表；`network.send_unicast` 由 change 06（NetworkService）提供

## Capabilities

### New Capabilities
- `intercom-voice-ptt`: Grouped 状态下的语音三态机（Idle/Talking/Listening/ChannelBusy）、半双工互斥、PTT press/release 全流程（clear-mode 50ms 仲裁 / free-mode 直通）、BOOT 与触摸双入口、熄屏直通、全局跨 App PTT 路由

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 voice.rs 行为 -->

## Impact

- **依赖**：05（AudioService 采集/播放/PA）、08（VOICE/TALK_STATE 包格式 `encode_voice`/`encode_talk_state`/`parse_talk_state`）、09（Grouped 状态、peer 列表、加密 ESP-NOW 通道）；`network.send_unicast` 由 change 06（NetworkService）提供
- **代码**：新增 `src/intercom/voice.rs`；在 `src/intercom/mod.rs` 注册 `pub mod voice;`；IntercomService 实现里 `Grouped(VoiceState)` 分支调用 voice 模块
- **输入路由**：InputService 的 `BootPress`/`BootRelease`/`BootShortTap` 事件需在 Shell 层路由到 IntercomService（全局，非 intercom_app 专属）；触摸 PTT 由 intercom_app 主页面触发 IntercomService::ptt_press/ptt_release
- **音频**：调用 AudioService::start_capture/stop_capture/pa_enable/on_capture_frame；就绪提示音与忙态提示音由 AudioService 播放
- **网络**：调用 NetworkService::send_unicast（change 06）发送 VOICE 包与 TALK_STATE 包（包构造复用 change 08 packet.rs `encode_voice`/`encode_talk_state`）
- **后续变更**：change 11（jitter/mixing）在本变更的 Listening 态接收链 standby 基础上接续实现解码/混音/播放；change 13（intercom-app-ui）在本变更的触摸 PTT 入口与 ChannelBusy UI 反馈上接续；change 14（voice-changer）在 on_capture_frame 回调里插入变声处理
- **无硬件变更**：BOOT=GPIO9、PA_CTRL=GPIO15 由 change 02/05 已落地
