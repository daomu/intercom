## 1. 模块骨架与 trait 定义

- [ ] 1.1 新增 `src/services/audio_service/mod.rs`：声明子模块，`pub use` 导出 `AudioService` trait、`AudioFrame`、`AudioError`、`MAX_OPUS_FRAME_SIZE` 常量
- [ ] 1.2 在 `src/services/audio_service/mod.rs` 定义 `pub trait AudioService: Send + Sync`，按技术设计 §3.4 落地全部方法签名（`start_capture`/`stop_capture`/`start_playback`/`stop_playback`/`on_capture_frame`/`submit_pcm`/`opus_decode`/`pa_enable`/`set_volume`/`set_mute`）；`on_capture_frame` 回调签名为 `Fn(&AudioFrame)`（按引用）；`opus_decode(&self, frame: Option<&[u8]>) -> Result<[i16; 320], AudioError>`（None=PLC，Some=正常解码）
- [ ] 1.3 定义 `pub struct AudioFrame { pub seq: u16, pub opus_data: [u8; MAX_OPUS_FRAME_SIZE], pub opus_len: usize }`，`MAX_OPUS_FRAME_SIZE = 160`
- [ ] 1.4 定义 `pub enum AudioError { I2sError, OpusError, BufferExhausted, InvalidParam }`，派生 `Debug/Clone/Copy`，实现 `std::error::Error` + `Display`
- [ ] 1.5 在 `src/services/mod.rs` 注册 `pub mod audio_service;`，移除占位 `#![allow(dead_code)]` 若需要

## 2. Opus 编解码器集成

- [ ] 2.1 验证 `audiopus = "0.3"` 在 `riscv32imc-esp-espidf` target 下 FFI 链接 libopus 静态库成功；若失败排查 `fixed-point` feature 传递
- [ ] 2.2 在 `src/services/audio_service/opus_codec.rs` 封装 `OpusEncoder` 包装：`new()` 创建 `Encoder::new(Hz16000, Mono, Voip)`，`encode(pcm: &[i16; 320], out: &mut [u8; 160]) -> Result<usize, AudioError>`
- [ ] 2.3 封装 `OpusDecoder` 包装：`new()` 创建 `Decoder::new(Hz16000, Mono)`，`decode(opus: &[u8], out: &mut [i16; 320]) -> Result<usize, AudioError>`；同时实现 `decode_plc(out: &mut [i16; 320]) -> Result<usize, AudioError>` 供 `opus_decode(None)` 调用
- [ ] 2.4 确认 `FIXED_POINT` 定点数模式已启用（编译 feature 或 cfg 传递），浮点 API 不可达

## 3. 采集链实现

- [ ] 3.1 新增 `src/services/audio_service/impl_default.rs`：定义 `pub struct AudioServiceImpl`，持有 I2S 采集句柄（来自 change 02）、I2S 播放句柄、PA_CTRL GPIO 句柄、OpusEncoder、OpusDecoder、预分配缓冲、混音槽、回调槽、音量/静音状态
- [ ] 3.2 实现 `AudioServiceImpl::new(i2s_rx, i2s_tx, pa_pin) -> Self`：在构造时一次性分配所有缓冲与 Opus 实例
- [ ] 3.3 实现 `start_capture`/`stop_capture`：启动/停止 I2S 采集通道；`start_capture` 不阻塞，采集循环由调用方 spawn
- [ ] 3.4 实现 `poll_capture_frame`（或同类内部方法）：从 I2S 读取 320 个 i16 → 经变声插入点（默认直通）→ Opus 编码 → 构造 `AudioFrame`（seq 单调递增）→ 触发 `on_capture_frame` 回调
- [ ] 3.5 实现 `on_capture_frame(cb)`：存储回调到内部 `Mutex<Option<Box<dyn Fn(&AudioFrame) + Send + Sync>>>`；采集帧池 2 个 `AudioFrame` 双缓冲轮转，回调借用帧 A 时采集填充帧 B
- [ ] 3.6 定义变声插入点 trait `VoiceEffectStage`（`fn process(&self, pcm: &mut [i16; 320])`），默认实现 `PassthroughStage` 为空操作；`AudioServiceImpl` 持有 `Option<&dyn VoiceEffectStage>` 槽位

## 4. 播放与混音链实现

- [ ] 4.1 定义 `struct MixSlot { pcm: [i16; 320], last_seq: u16, last_update_ms: u32, active: bool }`，在 `AudioServiceImpl` 中预分配 3 个 `MixSlot`
- [ ] 4.2 实现 `submit_pcm(src_id, pcm)`：按 `src_id` 索引到 `MixSlot`（>2 则丢弃最旧路），拷贝 PCM 到槽，标记 active
- [ ] 4.3 实现内部 `mix_and_play` 方法：遍历 active 槽 → 各路乘固定衰减系数 0.7 → 相加 → 软限幅器（tanh 类压缩，非硬削峰）→ 写入 I2S 播放缓冲；混音完全由 `submit_pcm` 内部拥有，jitter（change 11）仅解码并调用 submit_pcm
- [ ] 4.4 实现 `start_playback`/`stop_playback`：拥有 PA 软启动时序——start: `pa_enable(true)` → 等 1-2 帧 → 开始 I2S；stop: 零帧淡出 1-2 帧 → `pa_enable(false)` → 停止 I2S
- [ ] 4.5 实现 `pa_enable(on)`：纯 GPIO 翻转 PA_CTRL GPIO15（`BoardProfile::PA_CTRL_PIN`），不含任何时序/延时逻辑（BSP 提供 GPIO 句柄）
- [ ] 4.6 实现 `opus_decode(&self, frame: Option<&[u8]>) -> Result<[i16; 320], AudioError>`：`None` 调用解码器 PLC，`Some(data)` 调用正常解码；供 jitter（change 11）调用

## 5. 音量与静音

- [ ] 5.1 实现 `set_volume(v: u8)`：将 0-255 映射到衰减系数，存储到内部 `AtomicU8` 状态
- [ ] 5.2 实现 `set_mute(m: bool)`：存储到内部 `AtomicBool`，`mix_and_play` 时若 mute 则输出零帧
- [ ] 5.3 验证静音不切断 PA：`set_mute` 不调用 `pa_enable`，PA 状态独立

## 6. 资源池与零分配验证

- [ ] 6.1 审查全部实时路径代码（`poll_capture_frame`、`submit_pcm`、`mix_and_play`、I2S 读写、回调触发），确认无 `Vec::push`/`Box::new`/`String::from`/`vec!` 宏
- [ ] 6.2 确认 `AudioFrame`、`MixSlot`、PCM 缓冲均为栈内定长数组或构造时一次性分配的静态引用
- [ ] 6.3 估算总 SRAM 占用：采集 2×AudioFrame + 编码缓冲 + 解码缓冲 + 3×MixSlot + 播放缓冲 + Opus 内部状态，确认在 512KB 预算内

## 7. 构建验证

- [ ] 7.1 执行 `cargo build`，确认 `audiopus` FFI 链接成功，零编译错误
- [ ] 7.2 执行 `cargo build --release`，确认 release profile 通过，opt-level = "s" + LTO 无问题
- [ ] 7.3 执行 `cargo clippy`，修复本模块所有 warning

## 8. 设备验收

- [ ] 8.1 在 Waveshare ESP32-C6-Touch-LCD-1.54 上烧录含本模块的固件（需 change 02 I2S 初始化就绪）
- [ ] 8.2 验证采集链：调用 `start_capture` + `on_capture_frame` 回调，串口打印 `AudioFrame.seq` 与 `opus_len`，确认 seq 单调递增、opus_len > 0
- [ ] 8.3 验证播放链：构造本地 PCM 正弦波（440Hz）调用 `submit_pcm(0, pcm)` + `start_playback`，喇叭输出可听正弦音
- [ ] 8.4 验证 PA 软启动：`start_playback` 时喇叭无启动爆音/喀哒声；`stop_playback` 时无尾音硬切
- [ ] 8.5 验证音量/静音：`set_volume(50)` 明显低于 `set_volume(255)`；`set_mute(true)` 静音、`set_mute(false)` 恢复无延迟
- [ ] 8.6 验证多路混音：两路不同频率正弦波同时 `submit_pcm`，喇叭输出可辨两路混合音

## 9. 收尾

- [ ] 9.1 提交 commit：`feat: implement AudioService trait + Opus codec + PA soft-start (change 05/17)`
- [ ] 9.2 在 commit message 注明后续 change 10/11/14 将在此 AudioService 上扩展对讲业务、抖动缓冲、变声器
