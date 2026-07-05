## Why

17 个变更的第 8 个（依赖 change 01）。后续所有对讲业务（组队三阶段、语音 PTT、心跳恢复、变声、UI）都依赖统一的 ESP-NOW 包编解码与状态枚举。当前 `src/intercom/mod.rs` 仅有空占位，无任何包结构、序列化、状态机数据类型。需要先落地纯数据层 `packet.rs`：8 字节通用包头 + 9 种业务包类型的 encode/decode + IntercomState/HostPhase/JoinPhase/VoiceState/IntercomMode/VoiceEffect/IntercomEvent/IntercomError 全部枚举，使 change 09-13 能直接引用 `intercom::packet::*` 与 `intercom::state::*` 进行组队、语音、心跳业务逻辑开发。本期不涉及网络收发、加密计算、状态机迁移逻辑，仅为纯数据结构定义与零拷贝友好的序列化。

## What Changes

- 新增 `src/intercom/packet.rs`：定义 `PacketHeader`（8 字节固定布局：magic=0xC6 / ver / type / flags / seq:u16 BE / len:u16 BE）与 `PacketType` 枚举（0x01 VOICE / 0x02 HEARTBEAT / 0x03 TALK_STATE / 0x04 CTRL_BUSY_READY / 0x10 PAIR_BEACON_HOST / 0x11 PAIR_JOIN_REQ / 0x12 PAIR_JOIN_ACK / 0x20 DIRECTORY_BROADCAST / 0x30 CHANNEL_SWITCH_ACK）
- 在 `packet.rs` 为每种包类型定义 payload 结构体与 `encode`/`decode` 函数，严格按技术设计 §4.2-4.5、§5.2-5.6 的字段顺序与偏移实现：
  - `VoicePayload`（sender_id + effect + opus_payload 切片引用）
  - `HeartbeatPayload`（sender_id + state + mode）
  - `TalkStatePayload`（sender_id + action：1=start 0=end）
  - `CtrlBusyReadyPayload`（dst_id + code：1=busy 2=ready）
  - `PairBeaconHostPayload`（host_mac + host_pub_key + mode + cur_members + max_members + joinable）
  - `PairJoinReqPayload`（join_mac + join_pub_key + host_mac）
  - `PairJoinAckPayload`（host_mac + host_pub_key + join_mac + accepted + reason）
  - `DirectoryBroadcastPayload`（member_count + mode + target_channel + switch_offset:u16 BE + entries: N×38 字节 [mac+pub_key]）
  - `ChannelSwitchAckPayload`（sender_id + status）
- 新增 `src/intercom/state.rs`：定义技术设计 §3.5 全部枚举——`IntercomState`（含 Hosting/Joining/Grouped 子状态）、`HostPhase`、`JoinPhase`、`VoiceState`、`IntercomMode`（Clear/Free）、`VoiceEffect`（Normal/PitchUp/PitchDown）、`IntercomEvent`、`IntercomError`（含 `Net(NetError)` 变体引用 change 06 的 NetError）
- 在 `src/intercom/mod.rs` 注册 `pub mod packet; pub mod state;`
- 提供 `PacketHeader::parse` 与 `PacketHeader::encode` 零拷贝友好的字节级 API（输入/输出均为 `&[u8]` / `&mut [u8]`），不引入 `serde`/`bytes` 重依赖
- 包含 magic 校验、len 与 buf 长度一致性校验，违反时返回 `PacketError`（`BadMagic` / `BadVersion` / `Truncated` / `BadType` / `PayloadTooLarge`）。`PacketHeader::parse` 仅校验 magic==0xC6，返回含 `ver` 字段的 header 供调用方检查；ver 校验由 pairing 层（change 09）与非 pairing 层调用方负责

## Capabilities

### New Capabilities
- `intercom-packet`: ESP-NOW 通用包头与 9 种业务包类型的纯数据层编解码，无网络收发、无加密、无状态机
- `intercom-state`: 对讲业务状态枚举（IntercomState/HostPhase/JoinPhase/VoiceState/IntercomMode/VoiceEffect/IntercomEvent/IntercomError），供 UI 订阅与跨变更引用

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/intercom/packet.rs`、`src/intercom/state.rs`；升级 `src/intercom/mod.rs`（从空占位 `#![allow(dead_code)]` 改为注册两个子模块）
- **依赖**：`01`（仅引用 `BoardProfile` 的 `MAX_GROUP_SIZE`、`DISCOVERY_CHANNEL` 常量）；`NetError`（change 06 接口约定，本期仅 `use` 引用不实例化，若 change 06 未落地则用 `pub type NetError = u8;` 占位）；不引入 serde / bytes / heapless 等新 crate
- **后续变更**：change 09（组队三阶段）直接调用 `packet::encode_pair_beacon_host` 等；change 10（语音 PTT）调用 `packet::encode_voice` / `decode_voice`；change 12（心跳恢复）调用 `packet::encode_heartbeat`；change 13（UI）订阅 `IntercomEvent`、读 `IntercomState`
- **无运行时行为变更**：本期为纯数据结构与序列化函数，无 task 创建、无网络调用、无状态迁移；`main.rs` 不引用新模块
- **测试**：`#[cfg(test)]` 单元测试覆盖每种包类型的 encode→decode 往返、magic/ver 错误拒绝、边界长度校验
