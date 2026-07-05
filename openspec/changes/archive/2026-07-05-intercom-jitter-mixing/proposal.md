## Why

17 个变更提案的第 11 个（依赖 change 05、08、10）。change 10 已交付 PTT 三态机与 LISTENING 状态进入点，但接收端尚未实现抖动缓冲、序号去重与丢包隐藏能力——接收到 VOICE 包后无法按时序正确解码播放。本期在 `src/intercom/jitter.rs` 实现 per-sender 抖动缓冲 + Opus PLC + 弱网退化策略，使 LISTENING 状态下能稳定解码并提交各路 PCM 到 AudioService。混音由 AudioService（change 05）内部负责，本期不涉及。

## What Changes

- 新增 `src/intercom/jitter.rs`：per-sender 抖动缓冲结构（按 `sender_id` 分流，每路独立 ring buffer，初始水位 3 帧/60ms，动态最大水位防延时累积，硬顶 10 帧/200ms，floor 6 帧）
- 实现 seq 去重：`pkt.seq ≤ last_seen_seq[sender_id]` 立即丢弃，不进缓冲
- 实现 Opus PLC 丢包隐藏：超时未到的帧 → `audio_service.opus_decode(None)` 触发预测；连续丢包 > 4 帧/80ms → 静音封底（decoder 输出静音，非物理静音 padding）
- 解码委托 AudioService：jitter 不持有 Opus decoder 实例，所有解码调用 `audio_service.opus_decode(...)`；解码后按 sender 调用 `AudioService::submit_pcm(sender_id, &pcm)` 提交，混音由 AudioService 内部完成
- 时钟漂移轻量水位纠偏：长时间接收时按水位偏差微调，不做重型重采样
- 弱网退化优先级固化：1. 实时性 > 2. 音质下降 > 3. 限制路数 > 4. 抖动缓冲有上限 > 5. 极端情况局部丢字
- 禁止手动空白补偿：丢包/断帧时绝不填充物理静音或空白采样块，仅靠 Opus PLC 或 decoder 输出静音封底
- 全链路预分配固定缓冲与对象池，实时路径禁止高频动态分配
- 复用 change 05 的 `AudioService`（`opus_decode` + `submit_pcm`）、change 08 的 `packet.rs`（VOICE 包解析）与 change 10 的 LISTENING 入口；本期仅新增 jitter 逻辑层，不含混音

## Capabilities

### New Capabilities
- `intercom-jitter-mixing`: 接收端 per-sender 抖动缓冲、序号去重、Opus PLC 丢包隐藏（通过 AudioService opus_decode）与弱网退化策略；模块位于 `src/intercom/jitter.rs`

### Modified Capabilities
<!-- change 10 的 intercom-voice-ptt 行为不变，仅消费本能力提供的解码/混音输出 -->

## Impact

- **代码**：新增 `src/intercom/jitter.rs`；在 `src/intercom/mod.rs` 注册 `pub mod jitter;`；change 10 的 `voice.rs` LISTENING 接收回调接入 `jitter::on_recv_voice`
- **依赖**：依赖 change 05 的 `AudioService`（`opus_decode` + `submit_pcm`，混音由 AudioService 内部完成）、change 08 的 `packet.rs`（VOICE 包解析：seq + sender_id + opus_data）、change 10 的 `voice.rs`（LISTENING 状态与接收回调入口）
- **内存**：per-sender ring buffer 预分配（`[FrameSlot; 10]` × MAX_GROUP_SIZE，编译期固定），consecutive_lost 计数表，last_seen_seq 表——全部 `static` + `CriticalSection`/`Mutex` 保护，无 `static mut`
- **后续变更**：change 13（intercom-app-ui）的"正在发言" UI 显示依赖本能力的路数状态；change 17（integration-polish）的混音长稳测试覆盖本模块与 change 05 的混音层
- **无 PRD 规则变更**：本期为技术实现层，PRD §16.4-16.7/§13.2/§16.9 规则不变
