## Context

change 10 已交付 PTT 三态机（IDLE/TALKING/LISTENING）与接收回调入口：ESP-NOW 收到 VOICE 包后解密解析，调用 `on_recv_voice(pkt)`。但 `on_recv_voice` 当前是空壳——没有抖动缓冲、没有去重、没有 PLC、没有混音，无法应对网络抖动、乱序、丢包与多路并发。

技术设计 §10.2 / §10.5 / §19.5 与 PRD §16.4-16.7 / §13.2 / §16.9 已固化规则：per-sender jitter buffer（初始 3 帧/60ms）、seq 去重、Opus PLC（null 触发预测，连续 > 4 帧静音封底）、弱网退化优先级、禁止物理静音 padding、禁止高频动态分配。混音与限幅不在本期范围——AudioService（change 05）在 `submit_pcm` 内部完成固定衰减（0.7/路）+ soft limiter。

硬件约束：ESP32-C6 单核 160MHz、512KB SRAM、无 PSRAM；Opus 定点数解码单帧耗时需实测（技术设计 §21 列为待验证项）。MAX_GROUP_SIZE=4（BoardProfile）。

## Goals / Non-Goals

**Goals:**
- `src/intercom/jitter.rs` 实现 per-sender 抖动缓冲，按 `sender_id` 分流，初始水位 3 帧/60ms，动态最大水位（floor 6、硬顶 10）防延时累积
- seq 去重：`pkt.seq ≤ last_seen_seq[sender_id]` 立即丢弃
- Opus PLC：超时帧 → `audio_service.opus_decode(None)`；连续丢包 > 4 帧/80ms → decoder 输出静音封底
- 解码委托 AudioService：jitter 不持有 Opus decoder 实例；解码后调用 `AudioService::submit_pcm(sender_id, &pcm)` 提交，混音由 AudioService 内部完成
- 时钟漂移轻量水位纠偏（非重采样）
- 全链路预分配固定缓冲与对象池，实时路径零高频动态分配；`static` + `CriticalSection`/`Mutex` 保护，无 `static mut`
- 弱网退化优先级实现为可观测的策略层级，非 ad-hoc 补丁

**Non-Goals:**
- 重型重采样或 SRC 算法 → PRD 明确不做
- 物理静音 padding / 空白采样块填充 → PRD §16.4 明确禁止
- AEC / AGC / 侧音 / 全双工 → PRD §16.1 明确不做
- 立体声 / 双麦处理 → PRD §16.1 明确不做
- 动态混音路数自适应 UI 提示 → change 13 负责
- 混音 / 衰减 / limiter → AudioService（change 05）内部负责，本期不涉及
- Opus 编码路径 → change 05/10 已交付
- VOICE 包格式定义 → change 08 已交付
- LISTENING 状态机 → change 10 已交付

## Decisions

### D1：per-sender ring buffer，预分配定长数组
按 `sender_id`（组内 0..MAX_GROUP_SIZE-1 索引）分配独立 ring buffer，每个容量固定为 `[FrameSlot; 10]`（编译期硬顶，10 帧/200ms）。`last_seen_seq: [u16; MAX_GROUP_SIZE]`、`consecutive_lost: [u8; MAX_GROUP_SIZE]`、`ring: [JitterBuffer; MAX_GROUP_SIZE]` 全部用 `static`（非 `mut`）包裹，配合 `critical-section` crate 的 `CriticalSection` 或 `Mutex` 保护访问。备选：`static mut` → 不安全且 Rust 2024 edition 弃用，排除。备选：per-sender HashMap → 动态分配违反 PRD §16.9，排除。

### D2：初始水位 3 帧/60ms，首帧不立即播放
收到首帧后不立即解码播放，等水位 ≥ 3 才开始 pop。这用 60ms 延时换抖动吸收能力。备选：0 水位立即播放 → 首字丢失风险高，弱网下卡顿明显，排除。备选：5 帧水位 → 延时过大违反实时性优先级，排除。

### D3：动态最大水位 = max(6, 观测水位 + 2)，floor 6，硬顶 10
`max_water` 为运行时变量（≤ 10），按近期观测到的水位峰值 + 2 帧余量动态调整，但 floor 为 6、硬顶为 10。即 `max_water = clamp(max(6, observed + 2), 6, 10)`。超过 `max_water` 时 drop_oldest 强制出水，防延时无限拉长。`ring` 数组编译期固定 `[FrameSlot; 10]`，`max_water` 仅控制何时 drop oldest。备选：固定 10 帧 → 弱网下延时不可控，排除。备选：无上限 → 违反 PRD §16.6"动态上限"，排除。备选：无 floor → 弱观察期 max_water 可能降到 0 导致频繁 drop，排除。

### D4：seq 去重 = `pkt.seq ≤ last_seen_seq[sender_id]` 立即丢
严格 ≤ 比较（不是 <），重复包也丢。`last_seen_seq` 初始化为 0xFFFF（确保首帧 seq=0 不被误丢）。备选：滑窗去重（记录最近 N 个 seq）→ 内存开销高且 4 节点场景无必要，排除。

### D5：Opus PLC = `audio_service.opus_decode(None)`，连续 > 4 帧静音封底
超时未到的帧标记为 lost，调用 `audio_service.opus_decode(None)` 触发 libopus 内置预测。jitter 不持有 Opus decoder 实例——所有解码（含 PLC）委托 AudioService。`consecutive_lost` 计数：> 4（即 5 帧起/100ms）→ 跳过 PLC 直接输出 decoder 静音（PCM 全零），重置计数。备选：FEC / INBAND FEC → 编码端未启用且增加包大小，排除。备选：插值补偿 → CPU 开销高且主观连续性不如 libopus PLC，排除。

### D6：解码委托 AudioService，submit_pcm per sender
jitter 不做混音、不做衰减、不做限幅。每路正常帧解码调用 `audio_service.opus_decode(Some(frame.opus_data))`，PLC 调用 `audio_service.opus_decode(None)`，结果为 `[i16; 320]` PCM。随后调用 `AudioService::submit_pcm(sender_id, &pcm)` 将 per-sender PCM 送 AudioService。混音（固定衰减 0.7/路 + soft limiter + cap 3 routes）由 AudioService（change 05）内部完成。备选：jitter 内混音 → 职责越界，排除。

### D7：资源不足保留流策略 = 稳定性 × 持续性 评分
当活跃路数 > 3 或内存/CPU 吃紧时，按 `(连续收包率 × 收包持续时间)` 评分排序，保留 top-3，其余路降级为"不提交 submit_pcm 但 last_seen_seq 仍更新"（避免恢复后 seq 跳变）。备选：FIFO 保留最新 → 长稳流可能被淘汰，排除。备选：随机淘汰 → 不可控，排除。

### D8：时钟漂移 = 轻量水位纠偏，非重采样
长时间接收时若水位持续偏低（发送方时钟慢）→ 微调 drop 一帧；持续偏高（发送方时钟快）→ 微调插入一次 PLC（`audio_service.opus_decode(None)`）。纠偏频率 ≤ 每 10 秒一次。备选：SRC / 线性插值重采样 → CPU 开销高且 PRD §16.6 明确"不做重型重采样"，排除。

### D9：弱网退化优先级 = 策略层级 cascade
按 PRD §16.7 / 技术设计 §10.6 的 5 级优先级实现为有序降级链：
1. 实时性：max_water 硬顶 10 帧，超限 drop_oldest
2. 音质下降：PLC 优先，静音封底仅连续 > 4 帧
3. 限制路数：submit_pcm 路数封顶 3，资源不足按 D7 保留
4. 抖动缓冲有上限：D3 动态上限
5. 极端情况局部丢字：超硬顶后直接丢最旧帧
备选：单一阈值 → 无法分级降级，排除。

### D10：禁止物理静音 padding
PRD §16.4 明确"绝不填充物理静音或空白采样块"。本模块的"静音封底"是 decoder 输出全零 PCM（即 `consecutive_lost > 4` 时跳过 PLC 直接返回零数组），不是在 PCM 流中插入额外的静音帧。备选：插入静音帧对齐时间戳 → 违反 PRD，排除。

## Risks / Trade-offs

- **[Opus PLC 在定点数 RISC-V 的主观连续性]** → 技术设计 §21 列为待验证项；连续 4 帧阈值需实测调优，若主观体验差则调低阈值至 3 帧
- **[多路解码在无 PSRAM 设备的 CPU 占用]** → 多路 Opus 解码委托 AudioService，单核 160MHz 可能接近瓶颈；若不达标则降级为单路 + best-effort 第二路（submit_pcm 路数由 AudioService cap 3）
- **[动态 max_water 算法的稳定性]** → 水位观测窗口过短会导致 max_water 频繁波动；用 EMA 平滑 + floor 6/硬顶 10 钳位缓解
- **[时钟漂移纠偏误触发]** → 水位短期波动被误判为漂移；用 10 秒最小间隔 + 持续趋势确认缓解
- **[seq 回绕]** → u16 seq 在 65535 → 0 回绕时 ≤ 比较会误丢；用回绕检测（seq 差值 > 32768 视为回绕），备选：u32 seq → 包大小增加，排除
- **[资源不足保留流的 UI 一致性]** → 被降级的路 UI 仍显示"正在发言"但听不到；PRD §13.2 已明确"UI 显示 ≠ 一定能听到"，可接受
- **[对象池耗尽]** → 极端情况下 4 节点 × `[FrameSlot; 10]` × frame_size 可能逼近 SRAM 上限；用 BoardProfile::MAX_GROUP_SIZE=4 钳位 + ring 硬顶 10 缓解

## Migration Plan

无既有运行时需要迁移（change 10 的 `on_recv_voice` 是空壳）。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译
3. 烧录 2 台设备，一台 TALKING 一台 LISTENING，验证单路播放（jitter → opus_decode → submit_pcm → AudioService 混音输出）
4. 3-4 台设备自由模式并发 TALKING，验证多路 submit_pcm 经 AudioService 混音后 2 路稳定 / 3 路 best-effort
5. 模拟弱网（增大发送间隔 / 随机丢包），验证 PLC 与退化优先级

回滚：`git revert <commit>`，LISTENING 状态回到空壳（无播放）。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
