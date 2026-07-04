## Context

change 01 已建立工程骨架与 `BoardProfile` 常量（`OPUS_SAMPLE_RATE=16000`、`OPUS_FRAME_MS=20`、`MIC_CHANNELS=1`、`SPEAKER_CHANNELS=1`、`PA_CTRL_PIN=15`）。change 02（hal-bsp-drivers）提供 ES7210 I2S 采集句柄、ES8311 I2S 播放句柄、NS4150B PA_CTRL GPIO 句柄——本变更在此之上构建 `AudioService` trait 与默认实现。

技术设计 §3.4 定义 trait 签名，§10 定义音频流水线（发送链 §10.1、接收链 §10.2、资源池 §10.3、Opus 配置 §10.4、启停 §10.3/§16.2），PRD §16 给出能力边界（§16.1 单麦单声道、不做 AEC/AGC/侧音/全双工）、Opus 配置（§16.2 定点数）、启停规则（§16.8 预热采集链/避免包级频繁开关 PA/启停爆音抑制）、实现约束（§16.9 全链路预分配固定缓冲与对象池）。

三 task 并发模型（change 01 design.md D10）：Task A 音频实时优先级 19 / 栈 8KB。本变更的采集/编码/解码/混音/播放实时路径 SHALL 运行在 Task A 或由调用方（change 10/11）置于 Task A。

## Goals / Non-Goals

**Goals:**
- `AudioService` trait 与默认实现可被 change 10/11/14 直接 `use` 调用
- ES7210 采集输出 16kHz mono 20ms 帧（320 个 i16 sample），通过 `on_capture_frame` 回调上抛 `AudioFrame{seq, opus_data}`
- Opus 编码器/解码器以 `FIXED_POINT` 模式运行，16kHz/mono/20ms
- `submit_pcm(src_id, pcm)` 支持最多 3 路固定混音槽（良好 2 路、best-effort 3 路）
- PA 软启动：进入播放前拉高 PA_CTRL + 短启动缓冲；停止前淡出再拉低，避免爆音/喀哒声
- 全链路预分配固定缓冲与对象池，实时路径零高频动态分配
- `set_volume` / `set_mute` 生效于播放 DAC 软件衰减

**Non-Goals:**
- 网络发送/ESP-NOW/加密 → change 06/08/09
- 抖动缓冲策略与水位纠偏 → change 11
- 对讲状态机/PTT 流程 → change 10
- 变声器 DSP → change 14（本变更仅在采集链预留插入点）
- UI 反馈/状态订阅 → change 13
- AGC/AEC/侧音/全双工/波束成形 → PRD §16.1 明确不做
- 语音包分片 → PRD §16.3 一期默认不做

## Decisions

### D1：`AudioService` trait 签名 = 技术设计 §3.4 原样落地
trait 方法：`start_capture`/`stop_capture`/`start_playback`/`stop_playback`/`on_capture_frame(cb)`/`submit_pcm(src_id, pcm)`/`opus_decode(frame)`/`pa_enable(on)`/`set_volume(v)`/`set_mute(m)`。`on_capture_frame` 回调签名用 `Box<dyn Fn(&AudioFrame) + Send + Sync>`（按引用传递，配合 2 帧对象池双缓冲，trait object 避免泛型污染调用方）。`opus_decode(&self, frame: Option<&[u8]>) -> Result<[i16; 320], AudioError>`：`None` = PLC 丢帧补偿，`Some(data)` = 解码正常帧；jitter（change 11）调用此方法而非自带解码器。备选：`async` channel 上抛帧——增加 Task A 调度复杂度，排除。

### D2：`AudioFrame` 用固定数组而非 `Vec<u8>`
技术设计 §3.4 写 `opus_data: Vec<u8>`，但 PRD §16.9 禁止实时路径高频动态分配。落地为 `opus_data: [u8; MAX_OPUS_FRAME_SIZE]` + `opus_len: usize`（`MAX_OPUS_FRAME_SIZE = 160`，为 Opus 16kHz/mono/20ms VBR 峰值提供余量）。`AudioFrame` 由对象池预分配，回调借用 `&AudioFrame`。备选：保留 `Vec<u8>`——违反 PRD §16.9，排除。

### D3：采集/播放 I2S 驱动 = esp-idf-svc `I2sDriver` + 双缓冲
使用 `esp_idf_svc::i2s::I2sDriver`（change 02 已初始化 I2S 通道配置）。采集用 20ms 帧（320 samples × 2 字节 = 640 字节）双缓冲轮转；播放同理。I2S DMA buffer 按 2-4 个 frame 设计，兼顾延迟与抖动容错。备选：单缓冲 + 中断逐帧——CPU 中断负载高，排除。

### D4：Opus 编解码 = `audiopus` crate + `FIXED_POINT` feature
`audiopus = "0.3"`（change 01 已声明）。编码器/解码器在 `AudioService` 构造时各创建一个实例（`Encoder::new(SampleRate::Hz16000, Channels::Mono, Application::Voip)` / `Decoder::new(SampleRate::Hz16000, Channels::Mono)`）。启用 `FIXED_POINT` 编译配置关闭浮点 API（PRD §16.2）。备选：`opus-rs`——API 不如 `audiopus` 直观，排除。

### D5：混音 = 固定 3 槽 + 固定衰减 0.7 + 软限幅
`submit_pcm(src_id, pcm)` 按 `src_id` 索引到 3 个固定 `MixSlot`（每个含 320×i16 缓冲 + 最后更新时间戳）。混音算法：各路乘以固定衰减系数 0.7 → 相加 → 软限幅器（tanh 类压缩，非硬削峰到 [-32768, 32767]）。混音逻辑完全由 `submit_pcm` 内部拥有；jitter（change 11）仅按 sender 解码并调用 `submit_pcm`，不做衰减/混音。资源不足按稳定性保留流（丢弃最旧路）。备选：自适应权重或 1/N 均分衰减——计算开销大且 PRD 未要求，排除。

### D6：PA 软启动 = AudioService 拥有时序，pa_enable 为纯 GPIO
`start_playback`/`stop_playback` 拥有完整软启动时序。进入播放：① 先 `pa_enable(true)` 拉高 GPIO15（`pa_enable` 为纯 GPIO 翻转，委托 BSP，不含任何时序逻辑）② 等待 1-2 帧启动缓冲（20-40ms）③ 开始 I2S 输出。停止播放：① 输出零帧淡出 1-2 帧 ② `pa_enable(false)` 拉低 PA_CTRL ③ 停止 I2S。规则来自 PRD §16.8 与技术 §10.3/§16.2。备选：硬件 RC 滤波——Waveshare 板无此改造，排除。

### D7：资源池 = 编译期固定大小 + 对象池模式
- 采集帧池：2 个 `AudioFrame`（双缓冲轮转）——配合 D37 的 `&AudioFrame` 回调签名实现真正双缓冲：采集写入帧 A 时回调借用帧 B，反之亦然，无拷贝开销
- Opus 编码缓冲：1 个 `[u8; 160]`
- 解码 PCM 缓冲：1 个 `[i16; 320]`
- 混音槽：3 个 `MixSlot`
- 播放缓冲：1 个 `[i16; 320]`
所有缓冲在 `AudioServiceImpl::new` 时栈/静态分配一次，实时路径仅借用引用。备选：`heapless::Pool`——可考虑但增加依赖，先用裸数组。

### D8：`AudioError` 枚举
`AudioError { I2sError, OpusError, BufferExhausted, InvalidParam }`，`#[derive(Debug, Clone, Copy)]`，实现 `std::error::Error`。备选：`anyhow::Error` 透传——丢失类型化处理，排除。

### D9：变声插入点预留（不在本期实现）
采集链在「I2S 采集 → [变声插入点] → Opus 编码」之间预留一个可注入的处理节点接口（`VoiceEffectStage` trait 或函数指针槽），但本期默认直通。change 14 实现具体 DSP。备选：硬编码直通无插入点——change 14 需改流水线，排除。

### D10：采集/播放 task 归属 = 由调用方决定
本变更的 `AudioService` 实现自身不创建 task；采集/编码循环与解码/混音/播放循环由调用方（change 10/11）置于 Task A。`AudioService` 提供同步阻塞 API（`start_capture` 内部 spawn 或由调用方 spawn 由设计决定——本变更选「由调用方 spawn 并在 loop 中调用 `poll_capture_frame`」以保持 trait 简洁）。备选：内部 spawn——task 生命周期与 service 耦合，不利于 change 10/11 调度，排除。

## Risks / Trade-offs

- **[audiopus FFI 链接 libopus 静态库失败]** → 验证 `audiopus` 的 `fixed-point` feature 是否正确传递给 libopus 编译；若失败则改用 `opus-rs` 或自编译 libopus fixed-point（change 01 design.md 风险项已预留）
- **[ES7210 I2S 16kHz 采样率精度]** → ES7210 CODEC 内部 PLL 配置需由 change 02 正确初始化；若采样率漂移导致 Opus 编解码失真，回退到软件重采样（开销大，二期考虑）
- **[NS4150B PA 软启动时序与 Waveshare 板实际不符]** → 实测调整 `PA_SOFTSTART_FRAMES` 与淡出帧数；若仍有爆音，增加 RC 滤波硬件改造（二期）
- **[512KB SRAM 预算紧张]** → 全部缓冲占用约 4-6KB，可控；若 Opus 内部状态占用过大（约 20-40KB），需调低混音路数或帧长
- **[Task A 栈 8KB 是否足够 Opus 编解码]** → Opus 定点模式栈占用约 2-4KB，留余量；若溢出则调大栈或拆分编解码到独立 task
- **[混音 3 路在 160MHz RISC-V 上 CPU 占用]** → 实测；若 >30% CPU 则降级到 2 路（PRD §10.3 允许）

## Migration Plan

无既有运行时需迁移。部署步骤：
1. 拉取本变更 commit
2. 确保 change 02 的 I2S/PA 句柄接口已就绪
3. `cargo build` 验证 `audiopus` FFI 链接成功
4. 在 change 10 集成时调用 `AudioService::start_capture` 验证采集链
5. 在 change 11 集成时调用 `submit_pcm` 验证播放链

回滚：`git revert <commit>`，移除 `src/services/audio_service/` 模块。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
