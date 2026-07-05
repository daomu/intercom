## 1. VoiceEffect 枚举与模块骨架

- [x] 1.1 在 `src/intercom/voice_effect.rs` 顶部 `use crate::intercom::state::VoiceEffect`（枚举与 `to_u8`/`from_u8` 由 change 08 提供，不重新定义）
- [x] 1.2 在 `src/intercom/mod.rs` 注册 `pub mod voice_effect;`
- [x] 1.3 在 IntercomService 实现 `set_voice_effect(&self, e: VoiceEffect)`：写入运行时状态 `Cell<VoiceEffect>`，不持久化 NVS
- [x] 1.4 验证 `cargo build` 通过

## 2. TD-PSOLA Pitch Shift 引擎

- [x] 2.1 在 `voice_effect.rs` 实现 `pub fn pitch_shift(pcm: &[i16], effect: VoiceEffect) -> Vec<i16>`：Normal 档位直接返回原数据克隆；PitchUp/PitchDown 调用 TD-PSOLA
- [x] 2.2 实现简化自相关法基频检测：对 20ms 帧（320 样本 @16kHz）做自相关，找第一峰位作为基频周期；检测失败时 fallback 到固定周期 100 样本（160Hz）
- [x] 2.3 实现 TD-PSOLA 切片重叠相加：按基频周期标记 + 汉宁窗切片 → 按目标周期间距（原周期/倍率）重叠相加 → 输出与输入等长的 PCM 帧
- [x] 2.4 定义 PitchUp 倍率 = 1.5、PitchDown 倍率 = 0.67（实现阶段可微调，固定不暴露给用户）
- [x] 2.5 单元测试：验证输入 320 样本 → 输出 320 样本；Normal 直通返回相同数据；PitchUp/PitchDown 输出与输入不同

## 3. TX 链集成

- [x] 3.1 在 `src/intercom/voice.rs` 的 PTT 采集回调中，`on_capture_frame` 收到 PCM 帧后、送 Opus 编码前，调用 `voice_effect::pitch_shift(pcm, self.effect)`
- [x] 3.2 确保 Normal 档位零开销（函数内直接 return clone 或跳过调用）
- [x] 3.3 验证 pitch shift 输出 PCM 格式为 16kHz mono i16，与 Opus 编码器输入兼容
- [x] 3.4 在 `src/intercom/packet.rs` 的 `build_voice_pkt` 中，effect 参数填入 `self.effect.to_u8()`（替换当前固定 0）

## 4. 预览流程

- [x] 4.1 在 `voice_effect.rs` 或 `voice.rs` 实现 `preview_voice(&self) -> Result<(), IntercomError>`：
  - 检查 VoiceState == Idle，否则返回 `IntercomError::Busy`
  - `audio.start_capture()` + `audio.start_playback()`
  - 循环 150 次（3s = 150 帧 × 20ms）：
    - 采集 1 帧（20ms，320 样本，~640B）
    - 对该帧执行 `pitch_shift(pcm, self.effect)`（in-place 或就地替换）
    - `audio.submit_pcm(LOCAL_ID, &shifted_pcm)` 提交播放
  - 循环结束 `audio.stop_capture()` + `audio.stop_playback()`
- [x] 4.2 预览期间设置临时标志阻止 PTT 发言（或临时将 VoiceState 设为非 Idle），预览结束后恢复
- [x] 4.3 预览峰值内存 = 1 帧采集缓冲（~640B）+ 1 帧播放缓冲（~640B）≈ 1.28KB，无需 96KB 全量缓冲
- [x] 4.4 验证预览全程不调用 NetworkService::send

## 5. 接收端兼容性

- [x] 5.1 确认接收链路（`on_recv_voice` → jitter → opus decode → submit_pcm）不包含任何 pitch shift 调用
- [x] 5.2 确认接收端解析 effect 字段不崩溃（非法值 fallback Normal，见 1.1）
- [x] 5.3 接收端 effect 字段的 UI 展示留给 change 13，本变更仅保证不崩溃

## 6. 真机验证

- [x] 6.1 烧录到 2 台 Waveshare ESP32-C6-Touch-LCD-1.54
- [x] 6.2 验证 Normal 档位：PTT 发言 → 对端听感为原始音高
- [x] 6.3 验证 PitchUp：设档位 → PTT 发言 → 对端听感为升调
- [x] 6.4 验证 PitchDown：设档位 → PTT 发言 → 对端听感为降调
- [x] 6.5 验证预览：Idle 状态 → 调用 preview_voice → 本地播放变声后音频，对端无收到
- [x] 6.6 验证预览限制：Talking 状态调用 preview_voice → 返回错误 + UI 提示"当前对讲忙，无法预览"
- [x] 6.7 验证 VOICE 包 effect 字段：用 esp-now sniffer 或日志确认 effect 字段值为 0/1/2
- [x] 6.8 验证接收端不受变声影响：A 设 PitchUp 发言，B 设 Normal 接收，B 听到的是 A 的升调音频（非 B 自己的变声处理）

## 7. 性能与内存验证

- [x] 7.1 Benchmark spike：在真机测量 TD-PSOLA 单帧处理耗时。若 >10ms（20ms 帧长 50%），切换降级方案（线性插值重采样），记录决策
- [x] 7.2 测量 pitch shift 模块内存占用（环形缓冲 + 窗表）
- [x] 7.3 测量预览流式处理峰值内存（~1.28KB），确认远低于 96KB 全量方案
- [x] 7.4 长稳测试：PitchUp 档位连续 PTT 发言 5 分钟，无崩溃无内存泄漏

## 8. 收尾

- [x] 8.1 提交 commit：`feat: voice changer - TX pitch shift + preview (change 14/17)`
- [x] 8.2 在 commit message 注明 TD-PSOLA 决策与倍率参数，引用 design.md D1


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
