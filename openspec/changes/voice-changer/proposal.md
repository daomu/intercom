## Why

17 个变更提案的第 14 个。change 05 已提供 AudioService（采集/播放/混音），change 10 已提供 IntercomService 的 VoiceState 三态与 PTT 流程。当前 VOICE 包虽预留了 effect 字段（§4.2），但本机采集链路无 pitch shift 处理，变声档位仅停留在枚举定义层面。需要实现 TX 链 pitch shift、预览回放、effect 字段填充，使变声器从枚举变为可用的趣味增强能力。

## What Changes

- 新增 `src/intercom/voice_effect.rs`：复用 change 08 `src/intercom/state.rs` 中定义的 `VoiceEffect` 枚举（Normal=0 / PitchUp=1 / PitchDown=2，含 `to_u8`/`from_u8` 方法），本模块仅实现 pitch shift 引擎与预览流程，不重新定义枚举
- 在 TX 链（采集 → 编码前）插入 pitch shift 处理模块：当 `VoiceEffect != Normal` 时对 16kHz mono PCM 20ms 帧执行 TD-PSOLA 变调，Normal 档位直通零开销
- 实现 `set_voice_effect(e)`：持久化当前档位到运行时状态（后续 PTT 发言自动应用），effect 值随 VOICE 包 effect 字段发送
- 实现 `preview_voice()`：流式采集 3s（150 帧 × 20ms），逐帧 pitch shift 后立即提交播放队列；**不**发送群组；VoiceState ≠ Idle 时拒绝并提示"当前对讲忙，无法预览"
- VOICE 包封装时 effect 字段填入当前档位值（接收端不处理，仅展示用——本变更为发送端实现）
- 预览流程复用 AudioService 的采集与播放能力，独立缓冲区与 PTT 发言链路隔离

## Capabilities

### New Capabilities
- `voice-changer`: 变声档位枚举、TX 链 pitch shift 处理、本地预览回放（流式 3s 不发送）、VOICE 包 effect 字段填充

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/intercom/voice_effect.rs`（pitch shift 引擎 + 预览流程，复用 change 08 的 `VoiceEffect` 枚举）；修改 `src/intercom/mod.rs` 注册模块；修改 `src/intercom/voice.rs` 在 PTT 采集回调中插入 pitch shift 步骤；修改 `src/intercom/packet.rs` VOICE 包封装填入 effect 字段
- **依赖**：无新增第三方 crate；pitch shift 为纯 Rust 自实现 TD-PSOLA，不引入外部 DSP 库。变更依赖：`05, 08, 10`（08 提供 `VoiceEffect` 枚举与 `to_u8`/`from_u8` 方法）
- **内存**：pitch shift 需要额外环形缓冲（约 1-2 帧 PCM ≈ 320-640 样本 × 2B = 640B-1.28KB）；预览流式处理峰值内存 ≈ 1.28KB（1 帧采集 + 1 帧播放），在 512KB SRAM 预算内可接受
- **CPU**：TD-PSOLA 在 160MHz 单核上每 20ms 帧处理 320 样本，预计 < 1ms（Normal 直通为 0）
- **后续变更**：change 13（intercom-app-ui）的变声器页面将调用 `set_voice_effect` 与 `preview_voice`；change 17（integration-polish）做最终联调
