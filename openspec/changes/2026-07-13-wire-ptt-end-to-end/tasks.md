## 1. execute_voice_actions 实现

> 适配说明：实际 `VoiceAction`（`src/intercom/voice.rs`）无 `SendVoice/SetMuted/SetVolume`
> 变体；音量/静音走 settings 侧效果（wire-settings-side-effects），语音音频由音频线程
> 捕获/编码/发送（wire-audio-pipeline）。`execute_voice_actions` 处理真实变体：
> None/SendTalkState/StartCapture/StopCapture/PaEnable/ScreenOn/PlayReadyTone/
> PlayBusyTone/ArmCapture/DisarmCapture。

- [x] 1.1 在 `src/main.rs` 新增 `execute_voice_actions(actions, audio_svc, network_svc, talk_seq, display)` 函数
- [x] 1.2 处理 StartCapture/StopCapture → `audio_svc.start/stop_capture()`（含 PaEnable → `pa_enable`）
- [x] 1.3 处理 SendTalkState → `send_talk_state()`：`encode_talk_state` + peer 单播 fan-out（2B payload 适配 ESP-NOW 250B 上限）
- [x] 1.4 音量/静音无对应 VoiceAction 变体 → 由 settings 侧效果处理（wire-settings-side-effects，已完成）
- [x] 1.5 无 SendVoice 变体 → 语音由音频线程处理，`execute_voice_actions` doc 注释说明单一路径

## 2. PTT 触摸入口

- [x] 2.1 `hit_test` 命中 PTT 区域返回 `HitTarget::IntercomPttArea`（实际枚举名，等价 design 的 PttZone）
- [x] 2.2 `dispatch_touch` 处理 Down/Up on PTT 区 → `intercom_app.dispatch(Touch)` → `ptt.handle` 收集 VoiceAction
- [x] 2.3 触摸 dispatch 后调 `handle_ptt_outcome`（执行 actions + 调度仲裁/暖机定时）

## 3. BOOT 长按 PTT 入口

- [x] 3.1 `dispatch_button` 处理 `BootPress { screen_was_off }` → `intercom_app.dispatch(BootPress)` → `handle_ptt_outcome`
- [x] 3.2 处理 `BootRelease` → `intercom_app.dispatch(BootRelease)` → `handle_ptt_outcome`
- [x] 3.3 screen_was_off=true：VoicePttMachine 不发 ScreenOn（D8），controller 也不点亮屏幕（仅音频路径 arm）

## 4. 远端 TalkState UI 刷新

- [x] 4.1 `on_peer_voice_state(sender_id, talking)` 更新 `peers[i].voice_active` + Idle↔Listening 转换（已实现）
- [x] 4.2 peer 卡片 draw：`voice_active` 时绿色边框(2px) + "TALK" 文字（已实现于 intercom_view）
- [x] 4.3 主循环 drain `on_intercom_event` 返回的 VoiceAction → `execute_voice_actions`

## 5. 本地 PttState UI 反馈

- [x] 5.1 PTT 区按 `ui_state` 分支 draw：Idle(蓝灰)/PttArming(黄)/PttActive(红"RELEASE")/ChannelBusy(橙"BUSY")/Listening(绿)（已实现）
- [x] 5.2 render 仅在 intercom 前台派发（`launcher.foreground()==Intercom` → `intercom_app.render`）

## 6. ChannelBusy 降权

- [x] 6.1 `IntercomEvent::ChannelBusy` → `intercom_app.channel_busy = true`，主循环 2s 后 `clear_channel_busy()`
- [x] 6.2 PTT 区 ChannelBusy 时 draw 为橙"BUSY"（Rgb565 无 alpha，用橙色代替灰）；按 PTT 时 `log::warn` 提示但不阻止

## 7. 构建验证（用户自行在带工具链的机器执行）

- [ ] 7.1 `cargo build` 通过
- [ ] 7.2 `cargo test --lib` 通过（VoicePttMachine 单元测试不破坏）
- [ ] 7.3 双设备 `cargo run`：A 按 PTT，B 屏幕 A 卡片变绿 + 听到声音；B 按 PTT 反之（需 opus feature + 硬件整体验证）
