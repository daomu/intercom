## MODIFIED Requirements

### Requirement: controller 执行 VoiceAction
`src/main.rs` SHALL 新增 `execute_voice_actions(actions, audio_svc, network_svc, intercom_svc)` 函数，按变体派发：`StartCapture` → `audio_svc.start_capture()`；`StopCapture` → `audio_svc.stop_capture()`；`SendTalkState(ts)` → `network_svc.send(talk_state_pkt)`；`SetMuted(m)` → `audio_svc.set_mute(m)`；`SetVolume(v)` → `audio_svc.set_volume(v)`。SHALL NOT 丢弃任何 VoiceAction 返回值。`SendVoice` 变体 SHALL 不重复执行（音频线程已处理），记 log::warn。

#### Scenario: PTT 按下触发 capture
- **WHEN** 用户触摸 PTT 区 Down，`ptt.press()` 返回 `[StartCapture, SendTalkState(Talking)]`
- **THEN** controller 调 `audio_svc.start_capture()` + `network_svc.send(TalkState::Talking)`，本地麦克风开始采音 + 远端收到 "对方正在说话" 指示

#### Scenario: PTT 释放停止 capture
- **WHEN** 用户松开 PTT Up，`ptt.release()` 返回 `[StopCapture, SendTalkState(Idle)]`
- **THEN** controller 调 `audio_svc.stop_capture()` + `network_svc.send(TalkState::Idle)`，本地停止采音 + 远端 peer 卡片变灰

### Requirement: 触摸与 BOOT 双入口
IntercomApp SHALL 支持两个 PTT 入口：(a) 触摸 PTT 区 Down/Up → `dispatch_touch` → `ptt.press()/release()`；(b) BOOT 长按 + release → `dispatch_ptt_press/release`。两个入口 SHALL 共享同一个 `VoicePttMachine` 实例，第二个入口在已 Talking 时 SHALL 返回空 Vec（不重复 StartCapture）。

#### Scenario: 触摸 + BOOT 不双重触发
- **WHEN** 用户触摸 PTT 区正在 Talking，又长按 BOOT
- **THEN** BOOT 入口的 `ptt.press()` 返回空 Vec（state machine 已在 Talking 态），不重复调 `start_capture`

#### Scenario: 屏幕关时 BOOT PTT 仍工作
- **WHEN** 屏幕因 30s 超时熄灭，用户长按 BOOT
- **THEN** `InputDispatch::ForwardedPttPress { screen_was_off: true }` 触发，controller 调 `dispatch_ptt_press(true)`，audio_svc.start_capture 工作但 SHALL NOT 调 `display.screen_on()`（D8 spec：屏幕关时 PTT 仍工作且不点亮屏幕）

### Requirement: 远端 TalkState UI 刷新
主循环 drain `UiEvent::Intercom(TalkState { peer_id, state })` SHALL 调 `intercom_app.on_peer_voice_state(peer_id, state)`，更新对应 peer 的 voice_state。渲染时 peer 卡片 SHALL 根据 voice_state 显示"正在说话"指示（绿色边框 + 波形图标）。

#### Scenario: 远端 peer 按下 PTT
- **WHEN** 远端 peer A 按 PTT 发 TalkState::Talking，本端收到
- **THEN** `on_peer_voice_state(A, Talking)` 更新 `peers[0].voice_state = Talking`，下一 tick 渲染 A 卡片变绿色边框 + 波形图标，dirty=true 重绘

#### Scenario: 远端 peer 释放
- **WHEN** 远端 peer A 释放 PTT 发 TalkState::Idle
- **THEN** A 卡片恢复普通颜色，"正在说话"指示消失
