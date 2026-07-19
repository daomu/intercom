## Why

`IntercomApp.dispatch()` 返回的 `IntercomAppOutcome { voice_actions: Vec<VoiceAction> }` 被 main.rs 完全丢弃（change 13 已建好 VoicePttMachine + VoiceAction 枚举，change 12 已接通 dispatch，但 controller 侧未消费返回值）。导致：

1. **PTT 按下无 capture**：用户触摸 PTT 区或长按 BOOT，IntercomApp 切到 `PttState::Talking` 显示 UI 反馈，但 `audio_svc.start_capture()` 从未调用——本地麦克风没采音，没东西可发。
2. **PTT 释放无 stop**：同上，`stop_capture` 从未调，麦克风持续采样但没人 encode/send。
3. **TalkState 不广播**：用户按 PTT 应 `network_svc.send(TalkState::Talking)` 让远端 peer 看到"对方正在说话"指示；当前 VoiceAction::SendTalkState 被丢，远端看不到。
4. **VoicePacket 不发送**：VoiceAction::SendVoice(payload) 被丢，音频线程（change #3）虽然 encode 了，但没人投递到 network_svc。
5. **远端 TalkState 不影响 UI**：IntercomApp 已有 `on_peer_voice_state()` 方法，但 main loop 收到 TalkState IntercomEvent 时未调它，UI 不显示"对方正在说话"指示。

本期补齐 controller 侧对 VoiceAction 的执行 + UI 反向反馈。前置依赖 change #2（network_svc 可用）+ #3（audio_svc 可用）。

## What Changes

- **修改 `src/main.rs`**：新增 `execute_voice_actions(actions: Vec<VoiceAction>, audio_svc, network_svc, intercom_svc)` 函数，按变体调 audio_svc/network_svc/intercom_svc
- **修改 `src/apps/intercom_app.rs`**：`dispatch()` 与 `on_intercom_event()` 两个入口都返回 `Vec<VoiceAction>`；`on_peer_voice_state()` 返回 `Vec<VoiceAction>`（远端 TalkState 可能触发本端 ChannelBusy 视觉降权但不调 audio）
- **修改 `src/apps/intercom_app.rs`**：触摸 PTT Down/Up + BOOT long press/release 两个入口都映射到 `VoicePttMachine::press()` / `release()`，结果转 VoiceAction
- **修改 `src/apps/view/intercom_view.rs`**：draw 当前 `PttState` 反馈（Idle/Talking/Cooldown 三态颜色）+ 远端 peer 卡片 "正在说话" 指示（基于 on_peer_voice_state 更新的 state）

## Capabilities

### Modified Capabilities
- `intercom-voice-ptt`: VoiceAction 枚举 SHALL 由 controller 真正执行；VoicePttMachine.press/release 返回值 SHALL 不被丢弃
- `intercom-app-ui`: PTT 触摸区与 BOOT 长按 SHALL 都触发 VoicePttMachine；远端 TalkState SHALL 经 on_peer_voice_state 刷新 UI
