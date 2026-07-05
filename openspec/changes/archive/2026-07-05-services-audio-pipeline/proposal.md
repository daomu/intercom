## Why

17 个变更提案的第 5 个（change 05/17）。change 02 已提供 ES7210/ES8311/NS4150B 的 I2S 初始化与 PA_CTRL GPIO 句柄，但缺少上层音频服务：采集→编码→发送回调、接收 PCM 提交→混音→播放、Opus 定点编解码、PA 软启动防爆音、音量/静音控制、预分配固定缓冲与对象池。本期将 AudioService trait 与实现落地，为 change 10（PTT 语音对讲）与 change 14（变声器）提供可调用的音频流水线。

## What Changes

- 新增 `src/services/audio_service/mod.rs`：定义 `AudioService` trait（`start_capture`/`stop_capture`、`start_playback`/`stop_playback`、`on_capture_frame` 回调（按引用 `&AudioFrame`）、`submit_pcm` 混音输入、`opus_decode` 解码（含 PLC）、`pa_enable` 纯 GPIO 翻转、`set_volume`、`set_mute`）与 `AudioFrame` 结构体（`seq: u16` + `opus_data` 固定缓冲 `MAX_OPUS_FRAME_SIZE=160`）
- 新增 `src/services/audio_service/impl_default.rs`（或同名模块）：基于 change 02 暴露的 I2S 句柄实现 trait
- 采集链：ES7210 I2S 16kHz mono 20ms 帧（320 samples）→ 预分配采集缓冲（2 帧双缓冲）→ Opus 编码 → 通过 `on_capture_frame` 回调上抛 `&AudioFrame`
- 播放链：`submit_pcm(src_id, pcm)` 按 sender_id 分流到固定混音槽 → 各路 ×0.7 固定衰减 → 相加 → 软限幅（tanh 类压缩）→ ES8311 I2S 输出 → NS4150B PA；混音完全由 submit_pcm 内部拥有
- PA 软启动：`start_playback`/`stop_playback` 拥有软启动时序（pa_enable → 缓冲 → I2S 输出；零帧淡出 → pa_enable(false) → 停止 I2S）；`pa_enable` 为纯 GPIO 翻转
- Opus 配置：`audiopus` crate，`FIXED_POINT` 模式，16kHz / mono / 20ms frame；编码器与解码器实例各一，预创建；`opus_decode` 供 jitter（change 11）调用
- 资源池：采集/编码/解码/混音/播放全链路使用预分配固定缓冲与对象池；实时路径禁止高频动态分配
- 音量/静音：`set_volume(u8)` 影响播放 DAC 输出；`set_mute(bool)` 软静音（不切断 PA）
- 新增 `AudioError` 枚举（`I2sError`/`OpusError`/`BufferExhausted`/`InvalidParam`）

## Capabilities

### New Capabilities
- `audio-service`: AudioService trait 与默认实现，封装采集/编码/播放/混音/PA 控制/音量静音，提供 AudioFrame 结构与预分配资源池；不含对讲业务逻辑、网络发送、抖动缓冲策略（后者在 change 11）

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/services/audio_service/` 模块树；在 `src/services/mod.rs` 注册 `pub mod audio_service;`
- **依赖**：实际调用 `audiopus`（已在 change 01 声明）；调用 change 02 暴露的 I2S/PA 句柄接口
- **内存**：预分配采集帧缓冲（320×i16）、Opus 编码缓冲（≤单包上限）、解码 PCM 缓冲、混音槽（最多 3 路 × 320 samples）、播放缓冲；总占用在 512KB SRAM 预算内可控
- **后续变更**：change 10（PTT 语音）调用 `start_capture`/`stop_capture`/`on_capture_frame`；change 11（jitter/mixing）在 `submit_pcm` 之前接管抖动缓冲与混音决策；change 14（变声器）插入采集链变声处理节点
- **不涉及**：网络发送、ESP-NOW、加密、抖动缓冲策略、对讲状态机、UI
