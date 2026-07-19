## 1. HalAudioService 实例化

- [x] 1.1 构造 `HalAudioService::new(i2s, audio_in, audio_out)`（适配：沿用 display-service 所有权移交模式——在 `main.rs` 从 `hal` 移出 i2s/audio_in/audio_out 三字段传入，包成 `Arc<HalAudioService>`；`HalAudioService` 内部已用 `Mutex` 且 `Send+Sync`，无需外层 `Mutex`；`Hal` struct 定义保持不变，与 backlight/lcd 的处理一致）
- [x] 1.2 `HalAudioService::new` 接收三个真实句柄存为字段（change 05 已建）；新增 `capture_frame()`（I2S RX 读一帧 + effect_stage）+ `is_capturing()`/`is_playing()`；`set_volume`/`set_mute` 已实现（写 slots/atomic，混音时生效）；ES7210/ES8311 通过 `init_codec` 上电配置（在 main.rs 构造前调用）。注：寄存器级 DAC_VOL/mute 精调留待硬件
- [ ] 1.3 验证 `cargo build` 通过（需 ESP 交叉工具链，本环境无法编译）

## 2. 音频线程

- [x] 2.1 新增 `src/services/audio_service/thread.rs`：`AudioThread::spawn(audio_svc, rx_q, tx_sink, opus_enabled, loopback) -> JoinHandle`（适配：TX 发送经 `tx_sink` 回调解耦，network send 由 wire-ptt-end-to-end 接线；线程名 "audio"）
- [x] 2.2 线程主循环 TX：`audio_svc.is_capturing()` 时 `capture_frame()` 阻塞读 320 sample → `codec.encode` → `tx_sink`（loopback 时改为本地回放）
- [x] 2.3 线程主循环 RX：drain `rx_q: VoiceRxQueue`（`Arc<Mutex<VecDeque<VoicePacket>>>` cap=16，drop-oldest）→ `codec.decode` → `JitterMixer.submit_pcm` → `active_routes`（≤3 路）→ `audio_svc.submit_pcm`（内部 mix_and_play + I2S write）
- [ ] 2.4 I2S read/write 配置 DMA buffer 2×1024 sample，read 超时 50ms（当前 `read_pcm` 用 `BLOCK`；DMA/超时精调留待硬件）
- [x] 2.5 `start_capture` flag 由主线程 set（wire-ptt-end-to-end 接 PTT），音频线程读 `is_capturing()` flag

## 3. IntercomCodec

- [ ] 3.1 试 `cargo build --features opus` 看是否编译通过（需工具链；opus feature 默认 OFF）
- [x] 3.2 `src/intercom/codec.rs` 实现 `PcmPassThrough`：`encode(&[i16;320]) -> Vec<u8>`（16-bit LE bytecast）；`decode(&[u8]) -> [i16;320]`（zero-pad/truncate）；含 round-trip 单元测试
- [ ] 3.3 opus 可编译时用 `opus::Encoder`/`Decoder`（feature-gated 分支已留，FFI 接线留待工具链就绪）
- [x] 3.4 `IntercomCodec::new(opus_enabled: bool)` 工厂返回 enum（`PcmPassThrough` / feature-gated `Opus`）

## 4. JitterBuffer + 混音

- [x] 4.1 实例化 `JitterMixer::new()`（容量 = `BoardProfile::MAX_GROUP_SIZE`）于音频线程（change 11 已建 `jitter.rs`）
- [x] 4.2 `push(sender_id, seq, frame)` 写对应 peer ring buffer；`is_stale` 按 seq 去重 + wraparound 处理（jitter.rs 已实现）
- [x] 4.3 混音：`active_routes()`（按 route_score 排序、cap 3、清 pending）+ `AudioService::submit_pcm` → `mix_and_play`（0.7 衰减 sum + soft limiter）
- [x] 4.4 无活跃源 `mix_and_play` 输出静音（`active_count==0` → 0）；单源直接播放
- [x] 4.5 单元测试：codec round-trip + jitter mixer 路由/去重/水位/mix 幅度（jitter.rs + codec.rs 已有 `#[test]`）

## 5. 自检 loopback

- [x] 5.1 加 `AUDIO_LOOPBACK` 编译期 const flag（main.rs，默认 false 防 PA 啸叫）
- [x] 5.2 loopback 路径：capture → `codec` round-trip → `submit_pcm(0)` 播放（不发包），验证 I2S 双向
- [ ] 5.3 用户对着设备说话应听到自己延迟 ~80ms 后回来（需硬件）

## 6. 构建验证

- [ ] 6.1 `cargo build` 通过（需 ESP 交叉工具链）
- [ ] 6.2 `cargo test --lib` 通过（codec round-trip + jitter mix 单元测试）
- [ ] 6.3 `cargo run` 验证：loopback 模式听到自己回声；非 loopback 双设备 A→B（需 2 台硬件 + wire-ptt-end-to-end 完成）
