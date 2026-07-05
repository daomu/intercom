## ADDED Requirements

### Requirement: per-sender 抖动缓冲
模块 SHALL 为每个 `sender_id` 维护独立的 ring buffer 抖动缓冲，初始水位 3 帧/60ms——首帧到达后 SHALL NOT 立即解码播放，须等水位 ≥ 3 帧后才开始 pop 出帧。ring buffer SHALL 为编译期固定大小 `[FrameSlot; 10]`（硬顶 10 帧/200ms）。最大水位 `max_water` SHALL 为运行时变量，按 `max(6, observed + 2)` 计算，floor 为 6、硬顶为 10（`clamp(max(6, observed + 2), 6, 10)`），超过 `max_water` 时 SHALL drop_oldest 强制出水以防止延时累积。所有缓冲与计数表 SHALL 预分配固定内存（`static` + `CriticalSection`/`Mutex` 保护，无 `static mut`），运行时禁止高频动态分配。

#### Scenario: 首帧不立即播放
- **WHEN** LISTENING 状态下某 `sender_id` 的首个 VOICE 包到达
- **THEN** 包被推入该 sender 的 ring buffer，但不触发解码播放，等待水位 ≥ 3

#### Scenario: 初始水位达 3 帧后开始播放
- **WHEN** 某 `sender_id` 的 ring buffer 水位从 2 增至 3
- **THEN** 开始 pop 最旧帧并解码播放，此后每收到一帧 pop 一帧

#### Scenario: 动态最大水位防累积
- **WHEN** ring buffer 水位超过当前动态 `max_water`
- **THEN** drop_oldest 强制弹出最旧帧出水，水位回落到 `max_water` 以内

#### Scenario: floor 6 保底
- **WHEN** 观测水位为 2（observed + 2 = 4 < 6）
- **THEN** `max_water` 钳位为 6（floor），防止 max_water 过低导致频繁 drop

#### Scenario: 硬顶 10 帧
- **WHEN** 动态 `max_water` 计算值超过 10
- **THEN** `max_water` 钳位为 10，ring 数组编译期固定 `[FrameSlot; 10]`，防止延时无限拉长

#### Scenario: 预分配无动态分配
- **WHEN** 实时接收路径执行 `on_recv_voice`
- **THEN** 不发生 heap 分配，所有 ring buffer 槽位与计数表为预分配 `static`（+ `CriticalSection`/`Mutex`）内存，无 `static mut`

### Requirement: 序号去重
模块 SHALL 维护 `last_seen_seq[sender_id]` 表（初始化为 0xFFFF）。收到 VOICE 包时若 `pkt.seq ≤ last_seen_seq[sender_id]`（考虑 u16 回绕：seq 差值 > 32768 视为回绕不丢），SHALL 立即丢弃该包，不进 ring buffer。否则 SHALL 更新 `last_seen_seq` 并推入缓冲。

#### Scenario: 旧包丢弃
- **WHEN** 收到 `pkt.seq = 5` 且 `last_seen_seq[sender_id] = 8`
- **THEN** 包被立即丢弃，不进 ring buffer，不触发解码

#### Scenario: 重复包丢弃
- **WHEN** 收到 `pkt.seq = 8` 且 `last_seen_seq[sender_id] = 8`
- **THEN** 包被立即丢弃（≤ 比较含等号）

#### Scenario: 首帧 seq=0 不被误丢
- **WHEN** `last_seen_seq[sender_id]` 初始值 0xFFFF 且收到 `pkt.seq = 0`
- **THEN** 回绕检测判定非回绕（差值 1 ≤ 32768），包被接受并更新 `last_seen_seq = 0`

#### Scenario: u16 回绕正确处理
- **WHEN** `last_seen_seq[sender_id] = 65535` 且收到 `pkt.seq = 0`
- **THEN** 回绕检测（差值 65535 > 32768）判定为回绕，包被接受并更新 `last_seen_seq = 0`

### Requirement: Opus PLC 丢包隐藏（委托 AudioService）
当 ring buffer pop 出的帧标记为 lost（超时未到）时，模块 SHALL 调用 `audio_service.opus_decode(None)` 触发 libopus 内置预测生成 PCM。模块 SHALL NOT 持有 Opus decoder 实例——所有解码（含 PLC 与正常帧）SHALL 通过 `audio_service.opus_decode(...)` 进行。模块 SHALL 维护 `consecutive_lost[sender_id]` 计数：连续丢包 ≤ 4 帧（80ms）时用 PLC 预测；连续丢包 > 4 帧时 SHALL 跳过 PLC 输出 decoder 静音（全零 PCM）并重置计数。收到正常帧时 SHALL 重置 `consecutive_lost = 0` 并调用 `audio_service.opus_decode(Some(frame.opus_data))`。

#### Scenario: 单帧丢包 PLC 预测
- **WHEN** pop 出的帧为 lost 且 `consecutive_lost = 0`
- **THEN** 调用 `audio_service.opus_decode(None)` 生成预测 PCM，`consecutive_lost` 递增为 1

#### Scenario: 连续 4 帧丢包仍用 PLC
- **WHEN** `consecutive_lost = 3` 且下一帧仍为 lost
- **THEN** 继续调用 `audio_service.opus_decode(None)`，`consecutive_lost` 递增为 4

#### Scenario: 连续第 5 帧丢包静音封底
- **WHEN** `consecutive_lost = 4` 且下一帧仍为 lost
- **THEN** 跳过 PLC，输出全零 PCM，`consecutive_lost` 重置为 0

#### Scenario: 正常帧恢复重置计数
- **WHEN** `consecutive_lost = 2` 且 pop 出正常帧
- **THEN** `consecutive_lost` 重置为 0，调用 `audio_service.opus_decode(Some(frame.opus_data))` 正常解码

### Requirement: per-sender PCM 提交（不做混音）
模块 SHALL NOT 执行混音、衰减或限幅。每路解码后的 PCM（`[i16; 320]`）SHALL 按 `sender_id` 调用 `AudioService::submit_pcm(sender_id, &pcm)` 提交到 AudioService，由 AudioService 内部完成固定衰减（0.7/路）+ soft limiter + cap 3 routes 的混音。资源不足（活跃路数 > 3）时模块 SHALL 按稳定性（连续收包率）与持续性（收包持续时间）评分保留 top-3 流，被降级的流 SHALL 继续更新 `last_seen_seq` 但不调用 `submit_pcm`。

#### Scenario: per-sender 提交不混音
- **WHEN** 某 sender 的帧解码完成
- **THEN** 调用 `AudioService::submit_pcm(sender_id, &pcm)` 提交，jitter 内不做任何衰减/求和/限幅

#### Scenario: 资源不足保留流
- **WHEN** 4 路 sender 活跃但资源仅够 3 路
- **THEN** 按稳定性 × 持续性评分保留 top-3，第 4 路不调用 `submit_pcm` 但 `last_seen_seq` 持续更新

### Requirement: 时钟漂移轻量水位纠偏
长时间接收时模块 SHALL 监测 ring buffer 水位的持续趋势：若水位持续偏低（发送方时钟慢于接收方），SHALL 微调 drop 一帧；若水位持续偏高（发送方时钟快于接收方），SHALL 微调插入一次 PLC（`audio_service.opus_decode(None)`）。纠偏频率 SHALL 不超过每 10 秒一次。模块 SHALL NOT 执行重型重采样（SRC）或线性插值。

#### Scenario: 水位持续偏低纠偏
- **WHEN** 某 sender 的水位在 10 秒内持续低于初始水位的 50%
- **THEN** drop 一帧以纠偏，10 秒内不重复触发

#### Scenario: 水位持续偏高纠偏
- **WHEN** 某 sender 的水位在 10 秒内持续接近 MAX_WATER
- **THEN** 插入一次 PLC 帧（`audio_service.opus_decode(None)`）以纠偏，10 秒内不重复触发

#### Scenario: 禁止重型重采样
- **WHEN** 检测到时钟漂移
- **THEN** 仅用水位纠偏（drop/insert 单帧），不调用任何 SRC 或线性插值算法

### Requirement: 弱网退化优先级
模块 SHALL 按以下有序优先级实现弱网降级链，不得跳级或乱序：(1) 实时性——max_water 硬顶 10 帧，超限 drop_oldest；(2) 音质下降——PLC 优先，静音封底仅连续 > 4 帧时触发；(3) 限制 submit_pcm 路数——cap 3，资源不足按稳定性保留流；(4) 抖动缓冲有上限——动态 max_water；(5) 极端情况局部丢字——超硬顶后直接丢最旧帧。模块 SHALL NOT 用长缓存换完整性，SHALL NOT 填充物理静音或空白采样块。

#### Scenario: 实时性优先于音质
- **WHEN** 弱网导致水位超 max_water
- **THEN** drop_oldest 保证实时性，即使牺牲该帧音质

#### Scenario: 音质优先于 submit_pcm 路数
- **WHEN** 弱网导致丢包但未超 max_water
- **THEN** 先用 PLC 保音质，不先削减 submit_pcm 路数

#### Scenario: 限制 submit_pcm 路数优先于抖动缓冲扩容
- **WHEN** 资源吃紧且活跃路数 > 3
- **THEN** 先削减 submit_pcm 路数到 3，不先扩大 max_water

#### Scenario: 禁止物理静音 padding
- **WHEN** 丢包或断帧发生
- **THEN** 不插入任何物理静音帧或空白采样块，仅靠 Opus PLC 或 decoder 输出静音封底

### Requirement: 接收回调接入
change 10 的 `voice.rs` LISTENING 接收回调 SHALL 调用 `jitter::on_recv_voice(pkt)`，由 jitter 模块负责去重、缓冲、PLC 与 per-sender PCM 提交。模块 SHALL NOT 在 jitter 内做混音——解码（`audio_service.opus_decode(...)`）后按 `sender_id` 调用 `AudioService::submit_pcm(sender_id, &pcm)`，混音由 AudioService 内部完成。

#### Scenario: 接收回调委托给 jitter
- **WHEN** LISTENING 状态下收到 VOICE 包
- **THEN** `voice.rs` 调用 `jitter::on_recv_voice(pkt)`，由 jitter 模块处理去重 → 缓冲 → PLC → 解码 → submit_pcm

#### Scenario: per-sender PCM 提交到 AudioService
- **WHEN** jitter 模块完成解码（通过 `audio_service.opus_decode`）
- **THEN** 调用 `AudioService::submit_pcm(sender_id, &pcm)` 将 per-sender PCM 送 AudioService，由 AudioService 内部混音后输出到 ES8311/NS4150B
