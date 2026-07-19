## Why

`HalAudioService`（change 05）+ `AudioService` trait + `IntercomCodec`（change 11 Opus）已建好，但未接线。当前音频栈状态：

1. **`HalAudioService::new(i2s, audio_in, audio_out)` 未调用**：main.rs 把 `i2s` / `audio_in` / `audio_out` 三字段存在 `Hal` struct 里但从未构造 `HalAudioService`。`start_capture` / `stop_capture` / `encode` / `decode` / `submit_pcm` / `set_volume` / `set_mute` 全部空。
2. **没有音频线程**：原 spec 设计 3-task 并发（audio/network/UI），但确认决策为**单主线程 + 1 音频线程**。当前根本没有音频线程，I2S 阻塞读会卡死主循环。
3. **`opus` feature OFF**：`Cargo.toml` 中 `opus` feature 默认关闭（担心 esp-idf toolchain 缺 libopus）。要么打开 feature 真编译 Opus，要么本期先跑通 PCM 裸流路径再开 Opus。
4. **RX 路径未接**：`IntercomService.on_recv()` 收到 VoicePacket 后无处可解 → 没人调 `codec.decode()` → 没人调 `audio_svc.submit_pcm()` → 远端声音永远播不出。
5. **TX 路径未接**：PTT 按下应 `audio_svc.start_capture()` → 音频线程 `encode` → `network_svc.send()`；当前 VoiceAction 被 main.rs 丢弃（change #4 范围）。

本期：(a) 构造 `HalAudioService`；(b) 启动音频线程（std::thread，I2S 阻塞读 + Opus encode + queue 到 network_svc）；(c) RX：drain VoicePacket → decode → submit_pcm → I2S 阻塞写播；(d) jitter buffer + 简单混音（change 11/12 已有 spec，本期接通）。

## What Changes

- **修改 `src/main.rs`**：`Hal` 拆出 `i2s` / `audio_in` / `audio_out` 三字段传给 `HalAudioService::new()` 构造 `Arc<Mutex<HalAudioService>>`；启动 `std::thread::Builder::spawn` 音频线程；构造 `IntercomCodec`（Opus）实例
- **新增 `src/services/audio_service/thread.rs`**：音频线程主循环：(1) `audio_svc.start_capture` 后 `I2sDriver::read` 阻塞取 PCM 帧；(2) `codec.encode()` → `NetworkEvent::Send(VoicePacket)`；(3) RX 队列 `Mutex<VecDeque<VoicePacket>>` drain → `codec.decode()` → `audio_svc.submit_pcm` → `I2sDriver::write` 播放
- **修改 `src/services/audio_service/mod.rs`**：`HalAudioService::new(i2s, audio_in, audio_out)` 接收三个真实句柄；`start_capture`/`stop_capture` 真正启用 ES7210；`set_volume` 调 ES8311 DAC；`set_mute` 调 PA_CTRL GPIO
- **修改 `src/intercom/codec.rs`**（若缺）：`IntercomCodec::encode(&[i16]) -> Vec<u8>` / `decode(&[u8]) -> Vec<i16>`；若 opus feature OFF，先用 `PcmPassThrough` 占位（PCM 16-bit 直传，验证路径打通后再开 Opus）
- **修改 `src/intercom/jitter.rs`**（change 12 已有 spec）：实例化 `JitterBuffer` + 简单混音（多 peer 同时说话时取最大值或 sum-clip）
- **修改 `Cargo.toml`**：评估打开 `opus` feature；若 libopus 未在 esp-idf toolchain，用 `audiopus` 纯 Rust 或保持 PCM 路径，本期不阻塞

## Capabilities

### Modified Capabilities
- `audio-service`: HalAudioService SHALL 真正实例化；音频线程 SHALL 启动并打通 capture→encode→send + recv→decode→playback 双向路径
