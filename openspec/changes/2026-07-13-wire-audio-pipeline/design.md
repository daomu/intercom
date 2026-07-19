## Context

`HalAudioService`（`src/services/audio_service/mod.rs`）实现 `AudioService` trait：`start_capture`/`stop_capture`/`encode`/`decode`/`submit_pcm`/`set_volume`/`set_mute`。change 05 已建好但 main.rs 从未构造实例。`Hal` struct 当前持有 `i2s: I2sDriver<'static, I2sBiDir>` / `audio_in: AudioInDriver` / `audio_out: AudioOutDriver` 三字段作为 public（change 03），需移交所有权给 `HalAudioService`。

ESP32-C6 单核 160MHz + 512KB SRAM 无 PSRAM。3-task 原设计（audio/network/UI）改为**单主线程 + 1 音频线程**（用户决策）。音频线程负责所有 I2S 阻塞 I/O + codec encode/decode，主线程负责 UI + network event 桥 + pairing 状态机。

Opus codec：esp-idf toolchain 不一定带 libopus；`audiopus_lite` 或 `opus-rs` 纯 Rust 实现可尝试。本期决策：**先打通 PCM 裸流路径**（48kHz/16-bit/单声道 ≈ 96KB/s，单 peer 可走 ESP-NOW 250kbps 极限边缘），验证 I2S 双向 + jitter + 混音管线，再切换到 Opus（约 32kbps 宽裕）。PCM 路径作为 fallback。

## Goals / Non-Goals

**Goals:**
- `HalAudioService` 实例化，三字段所有权移交
- 音频线程启动：`std::thread::Builder::new().name("audio").spawn`
- TX 路径：PTT→start_capture→I2S read→encode→network_svc.send
- RX 路径：network recv VoicePacket→drain→decode→jitter→mix→submit_pcm→I2S write
- jitter buffer + 简单混音（多 peer 同时说话）
- `set_volume` / `set_mute` 真正生效（ES8311 DAC + PA_CTRL GPIO15）
- 设备自检：本地 capture → 立即回放（loopback 模式）以验证 I2S 双向通畅

**Non-Goals:**
- Opus 真实编译（先 PCM 路径；Opus 切换留为子任务标记 TODO）
- 回声消除 AEC（esp-idf 提供 AFE，但本期不接，留为后续）
- 降噪算法（后续）
- 多 peer 智能混音策略（本期 sum-clip 够用）

## Design

### Hal 字段拆分

```
// 改 Hal::init() 末尾：
let audio_svc = HalAudioService::new(
    i2s,        // 移交 I2sDriver
    audio_in,   // ES7210
    audio_out,  // ES8311 + PA_CTRL
)?;
Ok(Hal {
    lcd, touch, buttons, backlight, battery, radio,
    audio_svc: Arc::new(audio_svc),
})
```

主线程持 `Arc<Mutex<HalAudioService>>` 共享给音频线程。

### 音频线程

```
std::thread::Builder::new()
    .name("audio".into())
    .spawn(move || {
        let mut codec = IntercomCodec::new(opus_enabled);
        let mut jitter = JitterBuffer::new();
        let mut frame_buf = [0i16; FRAME_SAMPLES];
        loop {
            // TX: 若 capturing，I2S read → encode → network_svc.send
            if audio_svc.is_capturing() {
                audio_svc.i2s_read(&mut frame_buf);
                let payload = codec.encode(&frame_buf);
                network_svc.send(VoicePacket{payload});
            }
            // RX: drain rx_q
            while let Some(vp) = rx_q.lock().pop_front() {
                let pcm = codec.decode(&vp.payload);
                jitter.push(vp.src_id, pcm);
            }
            // 播放：jitter 取出 + 混音 → submit_pcm → I2S write
            if audio_svc.is_playing() {
                let mixed = jitter.mix_all();
                audio_svc.submit_pcm(&mixed);
                audio_svc.i2s_write(&mixed);
            }
        }
    })?;
```

帧长度：20ms Opus 帧 @ 16kHz = 320 samples；48kHz = 960 samples。esp-idf I2S DMA buffer 通常 1024 samples，分 2 chunk。本期取 20ms @ 16kHz 单声道（320 samples × 2 byte = 640B/frame）。

### jitter + 混音

`JitterBuffer`（change 12 spec 已有）：每个 peer 一个 ring buffer（深度 4 帧 = 80ms），按 rtp-like seq 排序。`mix_all()` 取所有活跃 peer 的当前帧，sum 后 clamp 到 i16 范围。单 peer 直传不混音。

### opus feature 决策

子任务 1.2 评估：
- 试 `cargo build --features opus` 看是否报 libopus 缺失
- 若失败，试 `audiopus_lite` 或 `opus-rs` 纯 Rust crate
- 若都失败，本期用 `PcmPassThrough` 占位（不 encode/decode，PCM 直传），打开 tracking issue 后续切 Opus

### 自检 loopback

加 debug flag `LOOPBACK_MODE`：启动后 capture → 立即 submit_pcm 播放，不发包。用户对着设备说话应听到自己延迟 80ms 后回来。验证 I2S 双向 + ES7210/ES8311 配置正确。

## Risks

- **I2S 阻塞读卡死**：若 ESP32-C6 I2S DMA buffer 配置不当，`read` 可能阻塞 > 100ms，影响音频线程响应。配置 2 个 DMA buffer 各 1024 sample，read 超时 50ms。
- **PSRAM 缺失**：jitter buffer 多 peer × 4 帧 × 960 sample × 2 byte = ~8KB/peer，无 PSRAM 时 SRM 紧张。限制 jitter 深度 + 最大 peer 数 4。
- **混音溢出**：sum-clip 简单但失真；后续换 AEC + 自动增益。
- **opus feature 编译失败**：fallback PCM 路径，不阻塞本期。

## Dependencies

- 前置：`2026-07-13-wire-network-runtime`（network_svc.send/recv 必须可用）
- 阻塞：`2026-07-13-wire-ptt-end-to-end`（PTT VoiceAction 依赖 audio_svc.start_capture）
