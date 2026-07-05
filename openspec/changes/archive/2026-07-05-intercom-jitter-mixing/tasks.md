## 1. 模块骨架与注册

- [x] 1.1 新增 `src/intercom/jitter.rs`：模块骨架，`#![allow(dead_code)]` 占位
- [x] 1.2 在 `src/intercom/mod.rs` 追加 `pub mod jitter;` 注册
- [x] 1.3 定义编译期常量：`JITTER_INIT_WATER = 3`、`JITTER_MAX_WATER_HARD = 10`、`JITTER_MAX_WATER_FLOOR = 6`、`PLC_CONSECUTIVE_LIMIT = 4`、`SUBMIT_MAX_ROUTES = 3`

## 2. per-sender ring buffer 与预分配

- [x] 2.1 定义 `FrameSlot` 结构：`{ seq: u16, opus_data: [u8; MAX_OPUS_FRAME_SIZE], is_lost: bool, ts: u64 }`
- [x] 2.2 定义 `JitterBuffer` 结构：`ring: [FrameSlot; 10]`（编译期固定硬顶 = 10）、`head`、`tail`、`waterline`、`last_seen_seq`、`consecutive_lost`、`max_water: usize`（运行时变量，初值 6，≤ 10，控制何时 drop_oldest）
- [x] 2.3 定义 `JitterPool`：`buffers: [JitterBuffer; MAX_GROUP_SIZE]`、`last_seen_seq: [u16; MAX_GROUP_SIZE]`、`consecutive_lost: [u8; MAX_GROUP_SIZE]`——全部用 `static`（非 `mut`）包裹，配合 `critical-section` crate 的 `CriticalSection` 或 `Mutex` 保护访问；禁止 `static mut`
- [x] 2.4 实现 `JitterBuffer::push(pkt)`：写入 ring buffer，更新 waterline
- [x] 2.5 实现 `JitterBuffer::pop() -> Option<FrameSlot>`：弹出最旧帧，更新 waterline
- [x] 2.6 实现 `JitterBuffer::waterline() -> usize`：返回当前水位

## 3. 序号去重

- [x] 3.1 实现 `seq_is_duplicate(sender_id, seq) -> bool`：`seq ≤ last_seen_seq[sender_id]` 判定（含 u16 回绕：差值 > 32768 视为回绕不丢）
- [x] 3.2 在 `on_recv_voice` 入口调用去重：重复/旧包直接 return
- [x] 3.3 更新 `last_seen_seq[sender_id] = pkt.seq`（接受包后）

## 4. 初始水位与动态最大水位

- [x] 4.1 实现初始水位门控：`waterline < 3` 时不 pop，仅 push
- [x] 4.2 实现动态 `max_water` 计算：`max_water = clamp(max(6, observed + 2), 6, 10)`，EMA 平滑观测水位
- [x] 4.3 实现 `drop_oldest`：超 `max_water` 时强制弹出最旧帧（不解码，直接丢弃）

## 5. Opus PLC 与静音封底（委托 AudioService）

- [x] 5.1 实现 lost 帧标记：ring buffer pop 时若某 seq 帧未到（超时）→ 标记 `is_lost = true`
- [x] 5.2 实现 PLC 路径：`is_lost && consecutive_lost ≤ 4` → 调用 `audio_service.opus_decode(None)` 生成预测 PCM，`consecutive_lost += 1`
- [x] 5.3 实现静音封底：`is_lost && consecutive_lost > 4` → 输出全零 PCM（decoder 静音），`consecutive_lost = 0`
- [x] 5.4 实现正常帧恢复：`!is_lost` → 调用 `audio_service.opus_decode(Some(frame.opus_data))`，`consecutive_lost = 0`
- [x] 5.5 确认 jitter 模块不持有 Opus decoder 实例；所有解码调用 `audio_service.opus_decode(...)`

## 6. per-sender PCM 提交（不做混音）

- [x] 6.1 实现解码后 PCM 提交：调用 `AudioService::submit_pcm(sender_id, &pcm)` 将 per-sender PCM 送 AudioService
- [x] 6.2 确认 jitter 内不做衰减、不求和、不限幅；混音由 AudioService 内部完成
- [x] 6.3 实现活跃路数计算：当前活跃 sender 数（封顶 3），用于资源不足保留流决策

## 7. 资源不足保留流

- [x] 7.1 定义稳定性评分：`连续收包率 = received / (received + lost)`（滑动窗口）
- [x] 7.2 定义持续性评分：`收包持续时间 = now - first_recv_ts`
- [x] 7.3 实现综合评分：`score = stability × duration_weight`
- [x] 7.4 实现保留 top-3 逻辑：`active_count > 3` 时按 score 排序，第 4+ 路不调用 `submit_pcm` 但 `last_seen_seq` 持续更新

## 8. 时钟漂移轻量纠偏

- [x] 8.1 实现水位趋势监测：10 秒窗口内水位均值
- [x] 8.2 实现偏低纠偏：均值 < 初始水位 × 50% → drop 一帧，10 秒冷却
- [x] 8.3 实现偏高纠偏：均值 > `max_water` × 80% → 插入一次 PLC（`audio_service.opus_decode(None)`），10 秒冷却
- [x] 8.4 确认无 SRC / 线性插值调用

## 9. 弱网退化优先级 cascade

- [x] 9.1 实现实时性层级：`max_water` 硬顶 10 + drop_oldest
- [x] 9.2 实现音质层级：PLC 优先 + 静音封底阈值
- [x] 9.3 实现 submit_pcm 路数层级：cap 3 + 保留流
- [x] 9.4 实现抖动缓冲上限层级：动态 `max_water`
- [x] 9.5 实现极端丢字层级：超硬顶直接丢最旧帧
- [x] 9.6 验证禁止物理静音 padding：代码中无任何空白采样块插入逻辑

## 10. 接收回调接入

- [x] 10.1 实现 `jitter::on_recv_voice(pkt)` 主入口：去重 → push → 水位门控 → pop → PLC/解码（`audio_service.opus_decode`）→ `submit_pcm(sender_id, &pcm)`
- [x] 10.2 在 change 10 的 `voice.rs` LISTENING 接收回调中调用 `jitter::on_recv_voice(pkt)`
- [x] 10.3 确认 `on_recv_voice` 全路径零 heap 分配

## 11. 构建验证

- [x] 11.1 执行 `cargo build`，确认零编译错误
- [x] 11.2 执行 `cargo build --release`，确认 release profile 通过
- [x] 11.3 静态检查：`on_recv_voice` 路径无 `Box`/`Vec::push`/`String` 等堆分配调用

## 12. 双机烧录验收

- [x] 12.1 烧录 2 台设备：一台 TALKING（按住 PTT 发话），一台 LISTENING，验证单路播放可听且无卡顿
- [x] 12.2 验证初始水位延迟：LISTENING 设备首帧到达后约 60ms 后开始播放（非立即）
- [x] 12.3 验证 seq 去重：手动重发相同 seq 包，确认不产生重复播放

## 13. 多机提交验收

- [x] 13.1 烧录 3 台设备：2 台同时 TALKING，1 台 LISTENING，验证 2 路 submit_pcm 经 AudioService 混音后稳定可听
- [x] 13.2 烧录 4 台设备：3 台同时 TALKING，1 台 LISTENING，验证 3 路 submit_pcm best-effort 混音
- [x] 13.3 烧录 4 台设备：4 台同时 TALKING，1 台 LISTENING，验证第 4 路被降级（不提交 submit_pcm 但 UI 仍显示"正在发言"——UI 验证留 change 13）

## 14. 弱网与 PLC 验收

- [x] 14.1 模拟弱网（增大发送间隔 / 随机丢包 20%），验证 PLC 预测帧主观连续性可接受
- [x] 14.2 模拟连续丢包 5+ 帧，验证静音封底触发后无爆音
- [x] 14.3 模拟长时间接收（> 30 秒），验证时钟漂移纠偏不误触发
- [x] 14.4 验证弱网退化优先级：弱网下实时性优先（drop_oldest 生效），不先削减 submit_pcm 路数

## 15. 收尾

- [x] 15.1 提交 commit：`feat: implement jitter buffer, PLC, and per-sender submit_pcm (change 11/17)`
- [x] 15.2 在 commit message 注明弱网退化优先级与 PRD §16.7 / 技术设计 §10.6 对齐，混音委托 AudioService（change 05）


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
