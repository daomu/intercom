## MODIFIED Requirements

### Requirement: apply_effect 接口接通
`IntercomCodec`（或独立模块）SHALL 实现 `apply_effect(pcm: &[i16], effect: VoiceEffect) -> Vec<i16>`。Phase 1（本期）SHALL 用 placeholder 实现：Low=0.7 倍音量 / Normal=1.0 / High=1.3 倍，clamp 到 i16 范围。Phase 2（后续 change）SHALL 替换为真实 pitch shift 算法（PSOLA / phase vocoder）。VoiceChanger 预览 SHALL 经 apply_effect 处理 preview_buffer 后回放，SHALL NOT 直接回放原始 PCM。

#### Scenario: Low 档位音量降低
- **WHEN** preview_buffer 含样本 [1000, -2000, 3000]，apply_effect(buf, Low)
- **THEN** 返回 [700, -1400, 2100]（×0.7 后 clamp）

#### Scenario: High 档位音量增大并 clamp
- **WHEN** preview_buffer 含样本 [25000]，apply_effect(buf, High)
- **THEN** 返回 [32500 clamp 到 32767]

### Requirement: 预览 buffer SRAM 优化
preview_buffer SHALL 用 8kHz 单声道采样（24000 samples × 2 byte = 48KB），SHALL NOT 用 16kHz（96KB 超出 SRAM 预算）。预览回放时 SHALL 升采样到 16kHz 提交 audio_svc。preview_buffer SHALL 在 IntercomApp 构造时 alloc 一次容量，SHALL NOT 每次录制重新分配。

#### Scenario: 8kHz 录制 3s buffer 大小
- **WHEN** 用户点档位按钮触发 3s 录制
- **THEN** preview_buffer 容量 24000 samples（48KB），录制期间 push 满 24000 后 stop_capture

#### Scenario: 回放升采样
- **WHEN** apply_effect 处理完 8kHz buffer 后调 submit_pcm
- **THEN** audio_svc 接收 16kHz 升采样后的 48000 samples，I2S 用 16kHz 播放

### Requirement: 通话中变声实时应用（TODO）
音频线程（change #3）TX 路径 SHALL 在 encode 前调 `apply_effect(pcm, settings.voice_effect)` 实现通话中实时变声。本期 SHALL 留 TODO 注释标记此接入点，SHALL NOT 实现真实通话变声（仅预览路径接通）。

#### Scenario: 通话变声 TODO 标记
- **WHEN** 音频线程 encode 前
- **THEN** 代码 SHALL 含 `// TODO(voice-changer): let pcm = apply_effect(pcm, settings.voice_effect);` 注释，等后续 change 实现真实 pitch shift 后启用
