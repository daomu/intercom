## Context

change 05 已实现 AudioService trait（`start_capture` / `stop_capture` / `on_capture_frame` / `submit_pcm` / `pa_enable`），提供 16kHz mono 采集与播放、PCM 帧回调。change 10 已实现 IntercomService 的 VoiceState（Idle/Talking/Listening/ChannelBusy）与 PTT 流程，VOICE 包封装函数 `build_voice_pkt(seq, sender_id, effect, opus_data)` 已预留 effect 参数但当前固定传 0。

技术设计 §14 规定变声档位为 Normal/PitchUp/PitchDown 三档，仅作用于 TX 链（本机采集 → 编码前），不处理接收音频。预览方式为录 3s → 本地 DSP → 本地播放，不发送群组。预览限制为 VoiceState ≠ Idle 时拒绝。

VOICE 包格式（§4.2）effect 字段偏移 9、长度 1，接收端不处理仅展示。PRD §17 定位为趣味增强能力，非强制使用，非对讲主能力前置条件。

当前 TX 链（§10.1）：麦克风 → 采集缓冲(20ms) → [变声处理(可选)] → Opus 编码 → 封装 VOICE 包。本变更填补"变声处理(可选)"这一环节。

## Goals / Non-Goals

**Goals:**
- `VoiceEffect` 枚举（Normal/PitchUp/PitchDown）与 `u8` 双向转换可用
- TX 链在采集回调 → Opus 编码之间插入 pitch shift：PitchUp/PitchDown 档位生效，Normal 直通
- `set_voice_effect(e)` 持久化档位，后续 PTT 发言自动应用该档位
- `preview_voice()` 录 3s 本地 PCM → pitch shift → 本地播放，不发送群组
- 预览在 VoiceState ≠ Idle 时拒绝，返回错误并触发 UI 提示"当前对讲忙，无法预览"
- VOICE 包 effect 字段填入当前档位值

**Non-Goals:**
- 接收端变声处理（接收端不处理 effect，仅展示用）
- 变声器 UI 页面布局与交互（→ change 13 intercom-app-ui）
- 多档位参数可调（仅三档固定值）
- 变声效果的实时监听（预览为录后回放，非实时）
- pitch shift 算法的 FFT/相位声码器实现（本变更选定 TD-PSOLA，见 D1）

## Decisions

### D1：Pitch Shift 算法 = TD-PSOLA（时域基频同步重叠相加）

选定 TD-PSOLA 作为 pitch shift 实现方案。

**原理**：对 16kHz mono PCM 帧做基频估计 → 在基频周期标记处加窗切片 → 按目标基频周期间距重叠相加重组 → 改变音高但保持时长与音色特征。

**理由**：
- 时域操作，无需 FFT/IFFT，计算量低（160MHz 单核可轻松处理 320 样本/20ms 帧）
- 语音信号准周期特性好，TD-PSOLA 对元音/浊音段效果好
- 内存占用小：仅需 1-2 帧环形缓冲（≈ 640B-1.28KB）+ 窗函数表
- 输出仍为 16kHz mono PCM，可直接送 Opus 编码器

**备选**：
- 相位声码器（phase vocoder）：频域操作，质量好但需 FFT/IFFT，ESP32-C6 无硬件浮点/FFT 加速，CPU 与内存开销过大，排除
- 简单重采样（线性插值变速再重采样补偿时长）：实现最简但会改变音色（formant 偏移），听感不自然，排除
- 固定延迟切片重叠相加（无基频检测）：实现简单但切片边界处易产生伪影，质量不稳定，排除

**PitchUp/PitchDown 参数**：
- PitchUp：目标基频 = 原基频 × 1.5（升 5 个半音左右）
- PitchDown：目标基频 = 原基频 × 0.67（降 5 个半音左右）
- 具体倍率在实现阶段可微调，三档固定不暴露给用户

### D2：Pitch Shift 插入位置 = 采集回调内、Opus 编码前

在 `on_capture_frame` 回调中，收到原始 PCM 帧后先执行 pitch shift，再送 Opus 编码器。这样：
- pitch shift 作用于 PCM（线性音频），不破坏 Opus 压缩域
- 一处插入，PTT 发言与预览回放共用同一 pitch shift 函数
- Normal 档位直接跳过处理，零开销

### D3：预览流程 = 流式采集 → 逐帧 pitch shift → 流式播放

预览不使用 PTT 的采集链路（避免与正在进行的发言冲突），采用流式处理以最小化内存：
1. 检查 VoiceState == Idle，否则拒绝
2. 调用 AudioService::start_capture + start_playback
3. 循环 150 次（3s = 150 帧 × 20ms）：
   - 采集 1 帧（20ms，320 样本，~640B）
   - 对该帧执行 pitch shift（in-place 或就地替换）
   - submit_pcm 提交到播放队列
4. 循环结束后 stop_capture + stop_playback
5. 全程不调用 NetworkService::send，不发送群组

峰值内存 = 1 帧采集缓冲（~640B）+ 1 帧播放缓冲（~640B）≈ 1.28KB，而非 96KB 全量缓冲。

预览期间临时阻止 PTT（VoiceState 临时设为非 Idle 或用独立 flag），防止预览与发言同时进行。

### D4：effect 字段填充 = 运行时档位值

`build_voice_pkt` 调用时传入当前 `self.effect`（`VoiceEffect` 转 `u8`）。接收端解析 effect 字段但仅用于 UI 展示（如显示对方使用了变声），不做音频处理。本变更仅实现发送端填充，接收端展示由 change 13 实现。

### D5：VoiceEffect 持久化 = 运行时内存，不持久到 NVS

档位仅保存在运行时状态（IntercomService 内 `Cell<VoiceEffect>` 或等价），重启后恢复为 Normal。理由：变声为趣味功能，非用户核心设置，不值得增加 NVS 读写；若用户需要持久化可在 change 16（safety-diagnostics）或后续迭代中补充。

> VoiceEffect 不持久化到 NVS，重启后重置为 Normal。这是设计选择，非缺陷——变声器定位为趣味增强，非用户长期偏好。

### D6：VoiceEffect 枚举来源 = change 08

`VoiceEffect` 枚举（`Normal`/`PitchUp`/`PitchDown`）与 `to_u8`/`from_u8` 方法定义于 change 08 的 `src/intercom/state.rs`。本变更不重新定义该枚举，仅 `use` 导入。`src/intercom/voice_effect.rs` 模块仅包含 pitch shift 引擎与预览流程实现。

## Risks / Trade-offs

- **[TD-PSOLA 基频估计在嘈杂环境下不稳定]** → 使用简化自相关法做基频检测，检测失败时 fallback 到固定周期切片（如 100 样本周期，对应 160Hz）；质量略降但不崩溃
- **[pitch shift 引入额外延迟]** → TD-PSOLA 处理延迟约 1 帧（20ms），在 PTT 实时性要求内可接受；预览流程为离线处理无延迟问题
- **[512KB SRAM 内存压力]** → pitch shift 环形缓冲 ≈ 1.28KB + 预览流式处理仅需 1 帧采集 + 1 帧播放缓冲（~1.28KB），峰值内存远低于 96KB 全量缓冲方案
- **[升调/降调倍率主观效果待验证]** → 实现阶段在真机上 A/B 试听微调倍率，选定最自然值后固定
- **[预览采集与 PTT 采集的 start_capture 冲突]** → D3 已通过 VoiceState 检查 + 预览期间阻止 PTT 缓解；若 AudioService 不支持并发采集则预览独占采集链路直到 stop
- **[TD-PSOLA CPU 预算风险]** → TD-PSOLA 在 160MHz RISC-V 单核上的 CPU 预算需实测。若单帧处理 >10ms（超过 20ms 帧长 50%），降级为简单重采样（线性插值，音质下降但无 CPU 风险）

## Migration Plan

无既有变声实现需要迁移。部署步骤：
1. 合入 `voice_effect.rs` 模块
2. 修改 `voice.rs` PTT 采集回调插入 pitch shift 调用
3. 修改 `packet.rs` VOICE 包 effect 字段填充
4. 编译验证 `cargo build`
5. 真机验证：设 PitchUp → PTT 发言 → 对端听感为升调；预览流程录 3s 回放

回滚：`git revert <commit>`，effect 字段恢复固定 0，pitch shift 调用移除。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
