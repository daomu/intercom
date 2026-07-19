> **实现适配说明（Adapt-to-real-code）**
>
> 实现以 spec 需求意图为准，适配实际代码 API；过时的任务签名按下述适配处理。
> 勾选 `[x]` 表示实现/单元测试代码已完成；构建与双机硬件验证（8.1/8.2/8.4/6.3）
> 保留 `[ ]`，由使用者在具备 ESP-IDF 工具链的机器上执行 `cargo build/test/run` 验证。
>
> 关键适配：
> - **VoiceEffect 命名**：实际枚举是 `Normal/PitchUp/PitchDown`（`src/intercom/state.rs`），
>   非 design 的 `Low/Normal/High`。UI 三档按钮映射到 `[Normal, PitchUp, PitchDown]`。
> - **apply_effect 复用真实 PitchShifter**：`src/intercom/voice_changer.rs` 已存在完整
>   PSOLA pitch shift（`PitchShifter::process`）。`apply_effect` 直接按 320 帧驱动它，
>   而非 design 的“音量增益 placeholder”——真实变声优于占位实现。放在 `voice_changer.rs`
>   而非不存在的 `codec.rs`。
> - **VoiceAction 是 `Copy` 枚举**，无法携带 `Vec` 载荷。改用 unit 变体
>   `StartPreviewPlayback` / `StopPreviewPlayback`；controller 从 `intercom_app.preview_buffer()`
>   读取处理后的缓冲并 `submit_preview_buffer` 提交。
> - **原始 PCM capture tap 延迟**：change #3 的音频线程在线程内 encode，未把原始 PCM
>   暴露给主线程。`push_preview_frame` 已实现但当前未接线，预览播放为静音占位
>   （documented TODO，随硬件整体验证时接通）。
> - **实时播放 pacing** 是硬件时序问题：`submit_preview_buffer` 先一次性分帧提交作占位。
> - **通话中实时变声接入点已具备**：`audio_service` 的 `VoiceEffectStage` trait 即 TX 实时
>   变声挂载点（task 7 的 TODO 已由架构提供）。

## 1. VoiceChanger 子状态机

- [x] 1.1 在 `src/apps/intercom_app.rs` 新增 `VoiceChangerSubState { Idle, Recording{remain_ms, target}, Previewing{remain_ms, effect} }` 枚举
- [x] 1.2 IntercomApp 新增 `vc_state: VoiceChangerSubState` + `preview_buffer: Vec<i16>` 字段
- [x] 1.3 `tick_voice_changer(&mut self, dt_ms)` 推进倒计时，返回产生的 `Vec<VoiceAction>`（适配：直接返回 action 列表，Recording→0 切 Previewing、Previewing→0 切 Idle）
- [x] 1.4 单元测试：Idle → Recording → tick → Previewing → tick → Idle（`vc_full_cycle_idle_recording_previewing_idle`）

## 2. dispatch 命中档位按钮

- [x] 2.1 在 `src/apps/view/intercom_view.rs` hit_test 命中 3 档按钮返回 `HitTarget::VoiceEffectButton(VoiceEffect)`
- [x] 2.2 `tap_voice_effect(effect)`：
    - Idle → 切 Recording{remain_ms=3000, target=effect} + 返回 `[StartCapture]`
    - Previewing → `[StopPreviewPlayback, StartCapture]` + 切 Recording（新档位）
    - Recording → 忽略返回 `[]`（view 层 Recording 时不返回效果按钮命中）
    - 通话中（`in_call`）→ 拦截，返回 `[]`
- [x] 2.3 Recording tick 到 0 → `apply_effect` + 切 Previewing + `[StopCapture, StartPreviewPlayback]`
- [x] 2.4 Previewing tick 到 0 → 切 Idle + `[StopPreviewPlayback]`

## 3. view 渲染

- [x] 3.1 `draw_voice_changer_page(fb, app)` 按 vc_state 分支：
    - Idle: 3 档按钮（active 效果 YELLOW 高亮）+ "Tap effect to preview 3s"；通话中显示 "Cannot preview during call"
    - Recording: 3 档按钮 dim + 红色 "Recording... {s}s" + Cancel
    - Previewing: 3 档按钮 dim + "Preview {effect} {s}s" + Cancel
- [x] 3.2 Cancel 按钮在 Recording/Previewing 时显示，命中返回 `HitTarget::CancelVoicePreviewButton`

## 4. controller 路由

- [x] 4.1 在 `src/main.rs` 处理新 VoiceAction 变体：`StartPreviewPlayback` → `audio_svc.start_playback()` + `submit_preview_buffer`；`StopPreviewPlayback` → `stop_playback()`（适配：unit 变体 + 从 model 读 buffer）
- [x] 4.2 tick 推进 vc_state：主循环 `intercom_app.tick_voice_changer(50)` 收集并执行产生的 VoiceAction
- [x] 4.3 通话中（`in_call`）进 VoiceChanger 页禁用按钮 + 显示 "Cannot preview during call"

## 5. apply_effect 实现

- [x] 5.1 在 `src/intercom/voice_changer.rs`（非不存在的 codec.rs）实现 `apply_effect(pcm: &[i16], effect: VoiceEffect) -> Vec<i16>`
- [x] 5.2 复用真实 `PitchShifter` PSOLA pitch shift（优于 design 的增益 placeholder）；`Normal` 直通
- [x] 5.3 单元测试：`apply_effect_normal_is_passthrough` / `apply_effect_empty_is_empty` / `apply_effect_preserves_length_across_frames`

## 6. preview_buffer SRAM 优化

- [x] 6.1 录制目标 8kHz 单声道（`VC_PREVIEW_SAMPLES = 24000`，≈48KB）；原始 PCM tap 待随音频线程整体接通（documented TODO）
- [x] 6.2 preview_buffer 用 `Vec::with_capacity(VC_PREVIEW_SAMPLES)`，`new()` 时 alloc 一次
- [ ] 6.3 验证 `cargo build` 后 SRAM 占用 < 80%（避免 OOM）— 需工具链

## 7. 通话中变声实时应用（留 TODO）

- [x] 7.1 TX 实时变声接入点已由 `audio_service` 的 `VoiceEffectStage` trait 提供（架构就位，TODO 已注释）
- [x] 7.2 本期不实现真实通话中变声，仅预览路径接通

## 8. 构建验证

- [ ] 8.1 `cargo build` 通过 — 需工具链
- [ ] 8.2 `cargo test --lib` 通过（vc_state 状态机 + apply_effect 单元测试）— 需工具链
- [x] 8.3 单元测试：Idle → Recording → Previewing → Idle 完整路径（含 `vc_recording_ignores_further_taps` / `vc_cancel_returns_to_idle` / `vc_blocked_during_call`）
- [ ] 8.4 `cargo run` 验证：进 VoiceChanger 页 → 点档位 → 录制 3s → 听到变声回放 → 回 Idle（需 change #3 完成后整体验证）
