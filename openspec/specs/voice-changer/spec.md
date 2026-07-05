## ADDED Requirements

### Requirement: VoiceEffect 枚举与档位定义
项目 SHALL 使用 change 08 `src/intercom/state.rs` 中定义的 `VoiceEffect` 枚举（含 `to_u8`/`from_u8` 方法），不重新定义。该枚举包含 `Normal`(值 0)、`PitchUp`(值 1)、`PitchDown`(值 2) 三个变体。该枚举 SHALL 用于 IntercomService 的变声档位设置与 VOICE 包 effect 字段填充。

#### Scenario: VoiceEffect 转 u8
- **WHEN** 将 `VoiceEffect::Normal` / `PitchUp` / `PitchDown` 转换为 `u8`
- **THEN** 结果分别为 0 / 1 / 2，与技术设计 §14.1 档位表一致

#### Scenario: u8 转 VoiceEffect
- **WHEN** 将 `u8` 值 0 / 1 / 2 转换为 `VoiceEffect`
- **THEN** 结果分别为 `Normal` / `PitchUp` / `PitchDown`

#### Scenario: 非法 effect 值容错
- **WHEN** VOICE 包解析到 effect 字段值为 3 或其他非 0/1/2 的值
- **THEN** 接收端将其视为 `Normal`，不崩溃不报错

### Requirement: TX 链 Pitch Shift 处理
当 `VoiceEffect` 不为 `Normal` 时，TX 链 SHALL 在采集回调 → Opus 编码之间对 16kHz mono PCM 20ms 帧执行 TD-PSOLA pitch shift 处理。`Normal` 档位 SHALL 直通 PCM 不做处理（零开销）。pitch shift 处理后的 PCM SHALL 保持 16kHz mono 格式以兼容下游 Opus 编码器。

#### Scenario: PitchUp 档位发言
- **WHEN** 用户设置 `VoiceEffect::PitchUp` 后执行 PTT 发言
- **THEN** 采集到的 PCM 经 TD-PSOLA 升调处理后送 Opus 编码，对端收到的语音音高高于原始

#### Scenario: PitchDown 档位发言
- **WHEN** 用户设置 `VoiceEffect::PitchDown` 后执行 PTT 发言
- **THEN** 采集到的 PCM 经 TD-PSOLA 降调处理后送 Opus 编码，对端收到的语音音高低于原始

#### Scenario: Normal 档位直通
- **WHEN** 当前 `VoiceEffect` 为 `Normal` 时执行 PTT 发言
- **THEN** 采集到的 PCM 不经过任何 pitch shift 处理，直接送 Opus 编码，与无变声模块时行为一致

### Requirement: set_voice_effect 持久化
`set_voice_effect(e)` SHALL 将档位保存到运行时状态，后续所有 PTT 发言 SHALL 自动应用该档位直到再次调用 `set_voice_effect`。档位 SHALL NOT 持久化到 NVS（重启后恢复 `Normal`）。

#### Scenario: 设置档位后发言
- **WHEN** 用户调用 `set_voice_effect(VoiceEffect::PitchUp)` 后按 PTT 发言
- **THEN** 该次及后续所有 PTT 发言均应用 PitchUp 档位，直到调用 `set_voice_effect` 切换

#### Scenario: 重启后恢复默认
- **WHEN** 设备重启后检查当前 VoiceEffect
- **THEN** 值为 `Normal`，重启前的设置不保留

### Requirement: 预览流程（流式 3s 本地播放）
`preview_voice()` SHALL 执行流式处理：采集 20ms 帧 → 执行当前档位的 pitch shift → 提交到播放队列 → 重复 150 次（共 3s）。预览 SHALL NOT 将音频发送到群组（不调用 NetworkService::send）。预览采集与播放 SHALL 使用 AudioService 的采集/播放能力。峰值内存 SHALL 不超过 1 帧采集缓冲 + 1 帧播放缓冲（~1.28KB）。

#### Scenario: Normal 档位预览
- **WHEN** 当前 VoiceEffect 为 Normal 时调用 `preview_voice()`
- **THEN** 流式采集 3s（150 帧），逐帧直通后播放原始音频，音频不发送群组

#### Scenario: PitchUp 档位预览
- **WHEN** 当前 VoiceEffect 为 PitchUp 时调用 `preview_voice()`
- **THEN** 流式采集 3s，逐帧升调处理后立即播放，音频不发送群组

#### Scenario: 预览不发送群组
- **WHEN** 预览流程执行期间
- **THEN** 不调用任何 NetworkService 发送方法，组内其他设备收不到预览音频

### Requirement: 预览状态限制
当 `VoiceState` 不为 `Idle`（即 Talking / Listening / ChannelBusy）时，`preview_voice()` SHALL 拒绝执行并返回错误。调用者 SHALL 向用户展示提示"当前对讲忙，无法预览"。

#### Scenario: 发言中拒绝预览
- **WHEN** VoiceState 为 Talking 时调用 `preview_voice()`
- **THEN** 返回错误（`IntercomError::Busy` 或等价），不启动采集，UI 提示"当前对讲忙，无法预览"

#### Scenario: 接收中拒绝预览
- **WHEN** VoiceState 为 Listening 时调用 `preview_voice()`
- **THEN** 返回错误，不启动采集，UI 提示"当前对讲忙，无法预览"

#### Scenario: 频道忙拒绝预览
- **WHEN** VoiceState 为 ChannelBusy 时调用 `preview_voice()`
- **THEN** 返回错误，不启动采集，UI 提示"当前对讲忙，无法预览"

#### Scenario: 空闲时允许预览
- **WHEN** VoiceState 为 Idle 时调用 `preview_voice()`
- **THEN** 预览流程正常启动，采集 3s 后本地播放

### Requirement: VOICE 包 effect 字段填充
VOICE 包封装时 effect 字段（偏移 9，长度 1）SHALL 填入当前 `VoiceEffect` 对应的 `u8` 值。接收端 SHALL NOT 对 effect 字段做音频处理（仅用于 UI 展示）。

#### Scenario: 发送端填充 effect
- **WHEN** 当前 VoiceEffect 为 PitchUp 时构建 VOICE 包
- **THEN** 包中 effect 字段值为 1

#### Scenario: 接收端不处理 effect
- **WHEN** 接收端收到 effect=1 的 VOICE 包
- **THEN** 接收端正常解码播放 Opus 音频，不因 effect 值做任何音频处理（effect 仅可用于 UI 展示，展示逻辑由 change 13 实现）

### Requirement: Pitch Shift 仅作用于 TX 链
pitch shift 处理 SHALL 仅作用于本机采集音频（TX 链），SHALL NOT 处理接收到的他人音频（RX 链）。接收链路从 ESP-NOW 解密到混音播放的全流程 SHALL 不包含任何 pitch shift 步骤。

#### Scenario: 接收音频不受变声影响
- **WHEN** 本机 VoiceEffect 为 PitchUp 且收到他人 VOICE 包
- **THEN** 接收音频按原始音高解码播放，不做 pitch shift 处理
