## Context

`VoiceEffect` 枚举（change 06）：`Low` / `Normal` / `High`（或 `Deep` / `Normal` / `Pitch`，命名看现存）。`Settings.voice_effect` 字段已存在。`IntercomCodec::apply_effect(&[i16], VoiceEffect) -> Vec<i16>` 接口在 change 11 spec 中要求，但实现状态未知——本期先用 placeholder（音量增益 / 简单抽样率变换）。

`IntercomPage::VoiceChanger` 页（`src/apps/view/intercom_view.rs`）已渲染 3 档按钮 + 当前选中高亮。spec §11 要求点击档位时先 3s 录制预览 → apply_effect → 回放，让用户听到效果。当前实现直接改字段无预览。

audio_svc 在 change #3 后可用（start_capture / submit_pcm）。jitter buffer 不参与预览（本地直接走 capture → buffer → apply_effect → submit_pcm）。

## Goals / Non-Goals

**Goals:**
- VoiceChanger 子状态机 Idle → Recording(3s) → Previewing(3s) → Idle
- 录制：3s @ 16kHz 单声道 = 48000 samples 存到 preview_buffer
- 预览：apply_effect 后 submit_pcm 回放，用户听到效果
- 用户在预览中点其他档位按钮 → 重新录制预览
- 录制中其他档位按钮禁用（hit_test 返回 None 或仅 Cancel）
- apply_effect placeholder 实现（音量增益 Low=0.7 / Normal=1.0 / High=1.3，后续换 pitch shift）

**Non-Goals:**
- 真实 pitch shift 算法（如 PSOLA / phase vocoder）——本期 placeholder，后续优化
- 变声器在其他场景的实时应用（PTT 通话中实时变声，spec 要求是通话中生效，但本期只验证预览路径；PTT 通话中应用 = `audio_svc.start_capture` 后自动 apply_effect，留 TODO）
- 预览 buffer PSRAM（ESP32-C6 无 PSRAM，96KB SRAM 紧张——用静态缓冲 + 限制 3s）

## Design

### 子状态机

```
VoiceChangerSubState {
    Idle,                   // 显示 3 档按钮，等用户点
    Recording { remain_ms: u32, target: VoiceEffect },  // 录制中，倒计时
    Previewing { remain_ms: u32, effect: VoiceEffect }, // 回放中，倒计时
}
```

### 流程

```
用户点 High 档位 (Idle → Recording)
    audio_svc.start_capture()
    每 50ms tick: remain_ms -= 50, 把 capture 的 frame push 到 preview_buffer
    remain_ms == 0 → stop_capture → apply_effect(buffer, High) → submit_pcm → Previewing { remain_ms: 3000, High }

Previewing 阶段
    每 50ms tick: remain_ms -= 50
    remain_ms == 0 → Idle

用户在 Previewing 阶段点 Low 档位
    stop_preview → start_capture → Recording { remain_ms: 3000, Low }
```

### apply_effect placeholder

```
fn apply_effect(pcm: &[i16], effect: VoiceEffect) -> Vec<i16> {
    let gain = match effect {
        VoiceEffect::Low => 0.7,
        VoiceEffect::Normal => 1.0,
        VoiceEffect::High => 1.3,
    };
    pcm.iter().map(|&s| ((s as f32 * gain) as i16).clamp(-32768, 32767)).collect()
}
```

真实 pitch shift 后续实现（rubberband / soundtouch 纯 Rust 不可用，需手写简单 PSOLA）。

### SRAM 预算

- preview_buffer: 48000 samples × 2 byte = 96KB
- ESP32-C6 SRAM 512KB，扣除系统 + 其他 buffer，96KB 偏紧
- 优化：录制到 8kHz 单声道 = 24000 samples × 2 = 48KB，预览时升采样到 16kHz 回放
- 或：preview_buffer 用 `&mut [i16; 24000]` static，限制 8kHz 录制

本期决策：8kHz 录制 + 48KB buffer + 升采样回放，降低 SRAM 压力。

### 布局

```
┌────────────────────────────┐
│  Voice Changer              │
│                              │
│  ┌─[Low]──┐ ┌─[Normal]─┐ ┌─[High]─┐ │  3 档按钮
│  │        │ │   [*]    │ │        │ │  当前选中高亮
│  └────────┘ └──────────┘ └────────┘ │
│                              │
│  [    Record 3s    ]          │  Idle 显示提示
│  或                          │
│  [● Recording... 2s]          │  Recording 倒计时
│  或                          │
│  [▶ Preview ▓▓▓░░░ 1.5s]     │  Previewing 进度条
│                              │
│  [ Cancel ]                  │  Recording/Previewing 时显示
└────────────────────────────┘
```

## Risks

- 96KB buffer SRAM 紧张：用 8kHz 录制降到 48KB
- apply_effect placeholder 仅音量变化，用户可能感觉不到效果差异——文档说明 "真实 pitch shift 待后续 change"
- 录制期间 PTT 通话冲突：用户在通话中切到 VoiceChanger 页录制预览会与通话 capture 冲突——禁止：通话中（ptt.state != Idle）进 VoiceChanger 页 SHALL 显示 "Cannot preview during call" + 禁用按钮

## Dependencies

- 前置：`2026-07-13-wire-audio-pipeline`（audio_svc 可用）
- 前置：`2026-07-13-wire-ptt-end-to-end`（避免通话中预览冲突）
- 不阻塞其他 change
