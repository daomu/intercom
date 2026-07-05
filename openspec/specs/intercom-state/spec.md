## ADDED Requirements

### Requirement: IntercomState 枚举
`src/intercom/state.rs` SHALL 定义 `IntercomState` 枚举，变体与技术设计 §3.5 1:1 对齐：`Idle`（未组网）、`Hosting(HostPhase)`、`Joining(JoinPhase)`、`Grouped(VoiceState)`。

#### Scenario: 后续变更模式匹配覆盖
- **WHEN** change 09 对 `IntercomState` 执行 `match`
- **THEN** 必须能匹配 `Idle` / `Hosting(HostPhase)` / `Joining(JoinPhase)` / `Grouped(VoiceState)` 四个变体，且子状态参数类型可读

### Requirement: HostPhase 枚举
`HostPhase` SHALL 定义四个变体：`Discovering`、`CollectingPeers`、`Frozen`、`SwitchingChannel`，与技术设计 §3.5 一致。

#### Scenario: 变体可构造
- **WHEN** 构造 `HostPhase::CollectingPeers`
- **THEN** 值可赋给 `IntercomState::Hosting(HostPhase::CollectingPeers)`

### Requirement: JoinPhase 枚举
`JoinPhase` SHALL 定义四个变体：`Searching`、`Requesting`、`WaitingConfirm`、`SwitchingChannel`，与技术设计 §3.5 一致。

#### Scenario: 变体可构造
- **WHEN** 构造 `JoinPhase::WaitingConfirm`
- **THEN** 值可赋给 `IntercomState::Joining(JoinPhase::WaitingConfirm)`

### Requirement: VoiceState 枚举
`VoiceState` SHALL 定义四个变体：`Idle`、`Talking`、`Listening`、`ChannelBusy`，与技术设计 §3.5 一致。该枚举 SHALL 为 `#[repr(u8)]` 以便 HEARTBEAT 包的 state 字段直接 `as u8` 编码。

#### Scenario: 数值可编码到 HEARTBEAT
- **WHEN** 调用 `encode_heartbeat` 传入 `state: VoiceState::Talking`
- **THEN** 写入偏移 9 字节等于 `VoiceState::Talking as u8`

### Requirement: IntercomMode 枚举
`IntercomMode` SHALL 定义两个变体：`Clear`（清晰模式）、`Free`（自由模式），与技术设计 §3.5 一致。该枚举 SHALL 为 `#[repr(u8)]` 以便 PAIR_BEACON_HOST / DIRECTORY_BROADCAST / HEARTBEAT 包的 mode 字段直接编码。

#### Scenario: 数值可编码到 PAIR_BEACON_HOST
- **WHEN** 调用 `encode_pair_beacon_host` 传入 `mode: IntercomMode::Clear`
- **THEN** 写入偏移 46 字节等于 `IntercomMode::Clear as u8`

### Requirement: VoiceEffect 枚举
`VoiceEffect` SHALL 定义三个变体：`Normal=0`、`PitchUp=1`、`PitchDown=2`，与技术设计 §3.5 与 PRD §17.2 一致。该枚举 SHALL 为 `#[repr(u8)]` 以便 VOICE 包的 effect 字段直接编码。该枚举 SHALL 提供 `fn to_u8(&self) -> u8`（返回变体数值）与 `fn from_u8(v: u8) -> Option<VoiceEffect>`（0→Normal, 1→PitchUp, 2→PitchDown，其他返回 None）方法，供 change 14 voice-changer 模块复用（D30：change 14 USE 此枚举不再重复定义）。

#### Scenario: 数值可编码到 VOICE
- **WHEN** 调用 `encode_voice` 传入 `effect: VoiceEffect::PitchUp`
- **THEN** 写入偏移 9 字节等于 `VoiceEffect::PitchUp as u8`

#### Scenario: to_u8 / from_u8 往返
- **WHEN** 调用 `VoiceEffect::PitchUp.to_u8()` 与 `VoiceEffect::from_u8(1)`
- **THEN** `to_u8()` 返回 `1`，`from_u8(1)` 返回 `Some(VoiceEffect::PitchUp)`；`from_u8(9)` 返回 `None`

### Requirement: IntercomEvent 枚举
`IntercomEvent` SHALL 定义变体与技术设计 §3.5 1:1 对齐：`StateChanged(IntercomState)`、`PeerOnline(u8, u8)`（id + rssi_4）、`PeerOffline(u8)`、`VoiceActive(u8)`、`ChannelBusy`、`PttReady`、`PairFailed(String)`。

#### Scenario: UI 订阅回调可收到事件
- **WHEN** change 13 的 UI 层注册 `subscribe(cb)` 回调
- **THEN** 回调参数类型为 `IntercomEvent`，可 `match` 全部 7 个变体

### Requirement: IntercomError 枚举
`IntercomError` SHALL 定义变体与技术设计 §3.5 1:1 对齐：`Busy`、`NotGrouped`、`ChannelBusy`、`InvalidState`、`Net(NetError)`。`NetError` SHALL 通过 `use crate::services::network::NetError;` 引用 change 06 的定义，不在本模块重复定义。

#### Scenario: 状态机返回错误
- **WHEN** change 09 在未组网状态调用 `ptt_press`
- **THEN** 返回 `Err(IntercomError::NotGrouped)`

#### Scenario: Net 变体承载 NetError
- **WHEN** NetworkService 返回 `NetError::TxFail`
- **THEN** IntercomService 可将其包装为 `IntercomError::Net(NetError::TxFail)` 上报

### Requirement: 模块注册
`src/intercom/mod.rs` SHALL 通过 `pub mod state;` 注册 state 模块，使其可被 `crate::intercom::state::*` 引用。

#### Scenario: 后续变更可引用
- **WHEN** change 09 在 `src/intercom/pairing.rs` 中 `use crate::intercom::state::{IntercomState, HostPhase}`
- **THEN** 编译期路径解析成功

### Requirement: 枚举可派生 Debug 与 Clone
`IntercomState`、`HostPhase`、`JoinPhase`、`VoiceState`、`IntercomMode`、`VoiceEffect`、`IntercomEvent`、`IntercomError` SHALL 派生 `Debug` 与 `Clone`，以便 UI 层日志输出与事件复制。

#### Scenario: Debug 输出可读
- **WHEN** 对 `IntercomState::Hosting(HostPhase::Frozen)` 调用 `format!("{:?}", ...)`
- **THEN** 输出包含 "Hosting" 与 "Frozen" 字样，无 panic
