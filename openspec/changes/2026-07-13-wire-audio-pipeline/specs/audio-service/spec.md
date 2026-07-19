## MODIFIED Requirements

### Requirement: HalAudioService 实例化与所有权移交
`Hal::init()` SHALL 在末尾调用 `HalAudioService::new(i2s, audio_in, audio_out)`，把 `I2sDriver` / `AudioInDriver` / `AudioOutDriver` 三字段所有权移交。`Hal` struct SHALL 改持 `audio_svc: Arc<Mutex<HalAudioService>>` 而非裸三字段。SHALL NOT 在移交后保留对原字段的引用。

#### Scenario: 三字段所有权移交
- **WHEN** `Hal::init()` 调 `HalAudioService::new(i2s, audio_in, audio_out)`
- **THEN** `Hal.audio_svc` 持有这三个句柄，原 `hal.i2s` / `hal.audio_in` / `hal.audio_out` 字段从 struct 移除，所有现存引用迁移到 `hal.audio_svc.lock().<method>()`

#### Scenario: 启动后 DAC/ADC 配置生效
- **WHEN** `HalAudioService::new` 完成构造
- **THEN** ES7210 ADC enable + ES8311 DAC enable + PA_CTRL GPIO15 输出方向 已配置；后续 `start_capture` 后立即可读 I2S

### Requirement: 音频线程双向路径
项目 SHALL 启动一个独立 `std::thread` 音频线程（名字 `"audio"`），负责所有 I2S 阻塞 I/O + codec encode/decode + jitter mix。主线程 SHALL NOT 直接调 `i2s_read` / `i2s_write`（避免阻塞 UI）。音频线程通过 `Arc<Mutex<HalAudioService>>` 与主线程共享 audio_svc，通过 `Arc<Mutex<VecDeque<VoicePacket, 16>>>` 接收远端包。

#### Scenario: TX 路径
- **WHEN** 主线程调 `audio_svc.start_capture()` 置 flag
- **THEN** 音频线程下一循环读到 flag，`i2s_read` 阻塞取 320 sample → `codec.encode` → `network_svc.send(VoicePacket)`，持续每 20ms 一帧直到 `stop_capture`

#### Scenario: RX 路径
- **WHEN** 主线程 drain network_q 收到 VoicePacket，push 到 `rx_q`
- **THEN** 音频线程下一循环 drain rx_q → `codec.decode` → `jitter.push` → `jitter.mix_all` → `submit_pcm` + `i2s_write` 播放

#### Scenario: I2S read 超时不卡死
- **WHEN** I2S DMA buffer 暂无数据（capture 尚未触发）
- **THEN** `i2s_read` 在 50ms 超时后返回，音频线程继续循环，SHALL NOT 永久阻塞

### Requirement: set_volume 与 set_mute 真正生效
`HalAudioService::set_volume(u8)` SHALL 调 ES8311 DAC_VOL 寄存器调整输出音量。`HalAudioService::set_mute(bool)` SHALL 调 ES8311 DAC mute + PA_CTRL GPIO15 拉低（mute 时 PA 关断省电）。SHALL NOT 仅写 `Settings` 持久化而不调硬件（这是 change #1 的非目标）。

#### Scenario: 静音硬件生效
- **WHEN** 用户长按 PLUS 触发 `set_mute(true)`
- **THEN** ES8311 DAC mute 位 = 1 + PA_CTRL GPIO15 = low（PA 关断），扬声器立即无声；`Settings.muted` 持久化 + 状态栏图标显示

#### Scenario: 音量调节硬件生效
- **WHEN** 用户在音量面板拖滑块从 50 到 80
- **THEN** ES8311 DAC_VOL 寄存器写对应值，扬声器音量立即变大；`Settings.volume` 持久化

### Requirement: JitterBuffer 与多 peer 混音
项目 SHALL 实例化 `JitterBuffer`（深度 4 帧 = 80ms / peer，最大 peer 4 个）。远端 VoicePacket 经 `codec.decode` 后 `push(peer_id, pcm)`，按 rtp-like seq 排序，过时帧丢弃。音频线程 `mix_all()` 取所有活跃 peer 当前帧 sum 后 clamp 到 i16 范围。单 peer 直传不混音。无活跃 peer 输出静音帧（全 0）。

#### Scenario: 多 peer 同时说话混音
- **WHEN** peer A 和 peer B 同时按 PTT 发声，jitter 各有活跃帧
- **THEN** `mix_all` 把两路 PCM sum 后 clamp 到 i16，扬声器听到两人声音叠加

#### Scenario: 过时帧丢弃
- **WHEN** jitter 收到 seq < 当前期望 seq 的包（迟到或乱序）
- **THEN** 该包被丢弃并 log::debug，SHALL NOT 插入到 ring buffer 错位置导致回放错位

### Requirement: Opus 与 PCM fallback
项目 SHALL 优先使用 Opus 编解码（16kHz 单声道 20ms 帧长，bitrate 32kbps）。若 esp-idf toolchain 缺 libopus 导致 `opus` feature 编译失败，SHALL fallback 到 `PcmPassThrough`（PCM 16-bit 直传，不 encode/decode）。fallback 路径 SHALL 在 250kbps ESP-NOW 速率下单 peer 可用（96KB/s PCM 超出，但若改为 8kHz 单声道则 16KB/s 可用，需在 fallback 时降采样率）。

#### Scenario: opus feature 编译失败 fallback
- **WHEN** `cargo build --features opus` 报 libopus 缺失
- **THEN** 项目默认构建走 PCM 路径，`IntercomCodec::new(opus_enabled=false)` 返回 PcmPassThrough，I2S 采样率降到 8kHz 单声道

#### Scenario: opus 可用走 Opus
- **WHEN** `opus` feature 编译成功
- **THEN** `IntercomCodec::new(true)` 用 `opus::Encoder/Decoder`，16kHz 单声道 20ms 帧，ESP-NOW 32kbps 单 peer 宽裕
