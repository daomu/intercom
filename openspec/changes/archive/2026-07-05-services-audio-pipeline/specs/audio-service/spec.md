## ADDED Requirements

### Requirement: AudioService trait 定义
项目 SHALL 在 `src/services/audio_service/` 下定义 `AudioService` trait，含以下方法：`start_capture(&self) -> Result<(), AudioError>`、`stop_capture(&self)`、`start_playback(&self) -> Result<(), AudioError>`、`stop_playback(&self)`、`on_capture_frame(&self, cb: Box<dyn Fn(&AudioFrame) + Send + Sync>)`、`submit_pcm(&self, src_id: u8, pcm: &[i16])`、`opus_decode(&self, frame: Option<&[u8]>) -> Result<[i16; 320], AudioError>`、`pa_enable(&self, on: bool)`、`set_volume(&self, v: u8)`、`set_mute(&self, m: bool)`。`on_capture_frame` 回调签名按引用传递 `&AudioFrame`（非按值），配合 2 帧对象池实现双缓冲：回调借用帧 A 时采集填充帧 B。`opus_decode` 的 `None` 参数表示 PLC（预测丢帧补偿），`Some(data)` 表示解码正常 Opus 帧；jitter（change 11）SHALL 调用此方法而非自带解码器。trait SHALL `impl Send + Sync`。

#### Scenario: 调用方可引用 trait
- **WHEN** change 10 在 `src/intercom/` 中 `use crate::services::audio_service::AudioService`
- **THEN** 编译期解析到 trait 定义，所有上述方法签名可见

#### Scenario: 采集启停对称
- **WHEN** 调用 `start_capture` 成功后再调用 `stop_capture`
- **THEN** I2S 采集通道停止，回调不再被触发，无资源泄漏

### Requirement: AudioFrame 结构
项目 SHALL 定义 `pub struct AudioFrame { pub seq: u16, pub opus_data: [u8; MAX_OPUS_FRAME_SIZE], pub opus_len: usize }`，其中 `MAX_OPUS_FRAME_SIZE = 160`（编译期常量，为 Opus 16kHz/mono/20ms VBR 峰值提供余量）。`AudioFrame` SHALL 不含堆分配字段。

#### Scenario: 回调上抛编码帧
- **WHEN** 采集一帧 20ms PCM 并经 Opus 编码后
- **THEN** `on_capture_frame` 回调被调用，参数为 `AudioFrame`，其 `seq` 单调递增，`opus_len` > 0 且 ≤ `MAX_OPUS_FRAME_SIZE`

#### Scenario: 无堆分配
- **WHEN** 在实时路径检查 `AudioFrame` 内存布局
- **THEN** 结构体仅含栈内定长数组与标量，无 `Vec`/`Box`/`String` 字段

### Requirement: ES7210 采集规格
采集链 SHALL 通过 ES7210 I2S 输出 16kHz / mono / 20ms 帧（320 个 `i16` sample）。采集缓冲 SHALL 预分配为固定大小，不在实时路径动态分配。

#### Scenario: 帧大小正确
- **WHEN** 采集一帧后检查 PCM 缓冲
- **THEN** 包含恰好 320 个 `i16` sample，对应 20ms @ 16kHz

#### Scenario: 采样率与 Opus 配置一致
- **WHEN** Opus 编码器接收采集 PCM
- **THEN** 采样率匹配 16000Hz，声道数匹配 mono，帧长匹配 20ms，无重采样开销

### Requirement: Opus 编解码配置
项目 SHALL 使用 `audiopus` crate 创建编码器与解码器各一实例，配置为 `SampleRate::Hz16000` / `Channels::Mono` / 20ms 帧 / `Application::Voip`，并启用 `FIXED_POINT` 定点数模式关闭浮点 API（PRD §16.2）。编码器与解码器实例 SHALL 在 `AudioService` 构造时一次性创建，实时路径仅复用。

#### Scenario: 定点数模式生效
- **WHEN** 检查 `audiopus` 编译 feature
- **THEN** `FIXED_POINT` 已启用，浮点 API 不可用或被禁用

#### Scenario: 编解码实例预创建
- **WHEN** `AudioService` 实现构造完成
- **THEN** 内部已持有可用 Encoder 与 Decoder 实例，实时路径无需再构造

### Requirement: 接收混音播放
`submit_pcm(src_id, pcm)` SHALL 按 `src_id` 路由到固定混音槽（最多 3 路活跃）。各路 PCM SHALL 乘以固定衰减系数 0.7 后相加，再经软限幅器（tanh 类压缩，非硬削峰）输出到 ES8311 I2S。混音槽与播放缓冲 SHALL 全部预分配，实时路径零动态分配。混音逻辑完全由 `AudioService::submit_pcm` 内部拥有；jitter（change 11）仅按 sender 解码 PCM 并调用 `submit_pcm`，SHALL NOT 在 jitter 层做衰减/混音/限幅。

#### Scenario: 单路播放
- **WHEN** 仅一路 `submit_pcm(0, pcm)` 持续提交 20ms PCM
- **THEN** ES8311 I2S 输出对应音频，无明显失真

#### Scenario: 多路混音不溢出
- **WHEN** 3 路同时 `submit_pcm` 提交满幅 PCM
- **THEN** 混音输出经限幅后不出现整数溢出回绕，无爆音

#### Scenario: 资源不足降级
- **WHEN** 4 路同时提交 `submit_pcm`
- **THEN** 系统按稳定性保留前 3 路，第 4 路被丢弃或静音，不崩溃

### Requirement: PA 软启动防爆音
`start_playback`/`stop_playback` SHALL 拥有 PA 软启动时序：`start_playback` 先 `pa_enable(true)` 拉高 PA_CTRL → 等待 1-2 帧启动缓冲 → 开始 I2S 输出；`stop_playback` 先输出零帧淡出 1-2 帧 → `pa_enable(false)` 拉低 PA_CTRL → 停止 I2S。`pa_enable(on)` trait 方法 SHALL 为纯 GPIO 翻转（委托 BSP），不含任何时序/延时/软启动逻辑。PA_CTRL 引脚 SHALL 为 GPIO15（`BoardProfile::PA_CTRL_PIN`）。

#### Scenario: 进入播放无爆音
- **WHEN** 从静默状态调用 `start_playback` 并立即有 PCM 提交
- **THEN** 喇叭无启动喀哒声或爆音，PA 在音频输出前已稳定

#### Scenario: 停止播放无尾音硬切
- **WHEN** 调用 `stop_playback` 时正在播放
- **THEN** 输出先淡出为静音，再关闭 PA，无尾音硬切爆音

#### Scenario: 避免包级频繁开关 PA
- **WHEN** 短时间内多包 PCM 间隔到达
- **THEN** PA 保持开启，不逐包开关

### Requirement: 音量与静音控制
`set_volume(v: u8)` SHALL 影响播放链的软件衰减系数（0=静音级，255=最大增益）。`set_mute(m: bool)` SHALL 软静音（输出零帧）但不切断 PA（PA 状态由 `pa_enable` 独立管理）。

#### Scenario: 音量生效
- **WHEN** `set_volume(128)` 后播放音频
- **THEN** 输出音量明显低于 `set_volume(255)` 时的音量

#### Scenario: 静音不切断 PA
- **WHEN** `set_mute(true)` 后再 `set_mute(false)`
- **THEN** PA 在静音期间保持原状态，恢复后无重新软启动延迟

### Requirement: 预分配资源池
采集/编码/发送回调/接收/解码/混音/播放全链路 SHALL 使用预分配固定缓冲与对象池。实时路径（采集帧回调、`submit_pcm`、混音、I2S 读写）SHALL NOT 调用 `Vec::push`/`Box::new`/`String::from` 等堆分配。采集帧池 SHALL 为 2 个 `AudioFrame` 对象池，配合 `&AudioFrame` 回调签名实现真正双缓冲：采集写入帧 A 时回调借用帧 B，反之亦然，无拷贝开销。

#### Scenario: 实时路径无堆分配
- **WHEN** 在采集回调与 `submit_pcm` 路径注入堆分配计数器
- **THEN** 每帧处理期间分配计数为零

#### Scenario: 缓冲在构造时分配
- **WHEN** `AudioService` 实现构造完成
- **THEN** 所有帧缓冲、Opus 实例、混音槽已就绪，运行时仅借用引用

### Requirement: AudioError 类型
项目 SHALL 定义 `AudioError` 枚举，至少包含 `I2sError`、`OpusError`、`BufferExhausted`、`InvalidParam` 变体，并实现 `std::error::Error` + `Debug` + `Clone` + `Copy`。

#### Scenario: 错误可类型化处理
- **WHEN** `start_capture` 返回 `Err(AudioError::I2sError)`
- **THEN** 调用方可 `match` 区分错误类型并采取不同恢复策略

### Requirement: 变声插入点预留
采集链 SHALL 在「I2S 采集 → Opus 编码」之间预留一个可注入的处理节点（trait 接口或函数指针槽），本期默认直通。该插入点 SHALL 不引入堆分配，且 SHALL 在编译期可禁用。

#### Scenario: 默认直通
- **WHEN** 本变更未注入变声处理器
- **THEN** 采集 PCM 原样进入 Opus 编码，无额外处理开销

#### Scenario: change 14 可注入
- **WHEN** change 14 实现变声器并注入处理节点
- **THEN** 采集 PCM 先经变声处理再编码，无需改动本变更的采集链骨架
