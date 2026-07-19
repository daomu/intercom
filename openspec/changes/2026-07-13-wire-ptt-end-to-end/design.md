## Context

`IntercomApp`（`src/apps/intercom_app.rs`，change 13）持有 `ptt: VoicePttMachine` 字段。`VoicePttMachine`（同 change）是状态机：`Idle → Talking → Cooldown → Idle`，`press()` 返回 `Vec<VoiceAction>`（含 StartCapture/SendTalkState::Talking），`release()` 返回含 StopCapture/SendTalkState::Idle 的 Vec。

`IntercomApp.dispatch()` 已返回 `IntercomAppOutcome { voice_actions: Vec<VoiceAction> }`。`on_intercom_event()` 返回 `Vec<VoiceAction>`。`on_peer_voice_state()` 返回 `Vec<VoiceAction>`。三处返回值当前都被 main.rs 丢弃。

`VoiceAction` 枚举（change 13）变体：
- `StartCapture` / `StopCapture` → audio_svc
- `SendTalkState(TalkState)` → network_svc.send
- `SendVoice(Vec<u8>)` → 已由音频线程处理（change #3），本 change 不重复
- `SetMuted(bool)` / `SetVolume(u8)` → audio_svc（部分场景）

## Goals / Non-Goals

**Goals:**
- controller 真正执行 `Vec<VoiceAction>` 返回值，按变体调对应 service
- 触摸 PTT 区 + BOOT 长按 两个入口都触发 VoicePttMachine.press/release
- 远端 TalkState 经 IntercomEvent 投递 → on_peer_voice_state → 刷新 peer 卡片
- UI 显示本地 PttState 反馈 + 远端 "正在说话" 指示
- ChannelBusy 时本地 PTT 触发降权（视觉变灰 + 短震动反馈，但不阻止）

**Non-Goals:**
- 音频 capture/encode/send 真实执行（change #3 范围）
- VoicePttMachine 状态机逻辑重写（change 13 已建好）
- VoicePacket 实际网络发送（change #3 音频线程处理）

## Design

### execute_voice_actions 函数

```
fn execute_voice_actions(
    actions: Vec<VoiceAction>,
    audio_svc: &Arc<Mutex<HalAudioService>>,
    network_svc: &EspNowNetworkService,
    intercom_svc: &mut IntercomService,
) {
    for a in actions {
        match a {
            VoiceAction::StartCapture => audio_svc.lock().start_capture(),
            VoiceAction::StopCapture => audio_svc.lock().stop_capture(),
            VoiceAction::SendTalkState(ts) => {
                let pkt = intercom_svc.encode_talk_state(ts);
                network_svc.send(broadcast_addr, pkt);
            }
            VoiceAction::SendVoice(_) => {
                // 已由音频线程处理，此处记 log::warn 说明双重路径
                log::warn!("SendVoice in VoiceAction — should be handled by audio thread");
            }
            VoiceAction::SetMuted(m) => audio_svc.lock().set_mute(m),
            VoiceAction::SetVolume(v) => audio_svc.lock().set_volume(v),
        }
    }
}
```

### PTT 触摸 + BOOT 双入口

`Launcher::dispatch_input` 已把 `BootPress { screen_was_off }` 路由到 `ForwardedPttPress`，`BootRelease` 路由到 `ForwardedPttRelease`。controller 收到后调：

```
let actions = intercom_app.dispatch_ptt_press(screen_was_off);
execute_voice_actions(actions, ...);

let actions = intercom_app.dispatch_ptt_release();
execute_voice_actions(actions, ...);
```

触摸 PTT 区：`intercom_view::hit_test` 命中 `HitTarget::PttZone` 时调 `intercom_app.dispatch_touch(Down{ptt_zone})` → `ptt.press()`；`Up` → `ptt.release()`。

### 远端 TalkState UI 刷新

`IntercomEvent::TalkState { peer_id, state }` → 主循环 drain → `intercom_app.on_peer_voice_state(peer_id, state)`。该方法更新 `peers` 字段中对应 peer 的 `voice_state` 标志，渲染时 peer 卡片显示"正在说话"指示（颜色变绿 + 波形图标）。

### ChannelBusy 降权

`IntercomEvent::ChannelBusy` 事件 → IntercomApp 设 `channel_busy = true`。UI 在 PttState::Idle 时把 PTT 区绘制为半透明灰（视觉降权）。用户仍可按 PTT（不阻止），但 main loop log::warn 提示冲突。

## Risks

- BOOT 长按 PTT 与触摸 PTT 同时触发：VoicePttMachine 应只响应第一个，第二个返回空 Vec（已在 change 13 state machine 实现，验证即可）
- 远端 TalkState 与本地 PTT 同时说话：本地 audio_svc.submit_pcm + 远端 jitter → 混音（change #3 处理）
- `SendVoice` 双重路径：audio_svc 线程已直接 encode + send，本 change 不再重复执行，记 log::warn

## Dependencies

- 前置：`2026-07-13-wire-network-runtime`（network_svc.send 可用）
- 前置：`2026-07-13-wire-audio-pipeline`（audio_svc.start_capture 可用）
- 阻塞：用户能真正用 PTT 完整对讲（与 #5 #6 #7 一起完成全流程）
