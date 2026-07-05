## 1. 模块注册与常量

- [x] 1.1 在 `src/intercom/mod.rs` 移除空 `#![allow(dead_code)]` 占位，注册 `pub mod packet;` 与 `pub mod state;`
- [x] 1.2 在 `src/intercom/packet.rs` 顶部定义常量：`const MAGIC: u8 = 0xC6;`、`const SCHEMA_VER: u8 = 1;`、各字段偏移常量（`OFF_MAGIC=0`、`OFF_VER=1`、`OFF_TYPE=2`、`OFF_FLAGS=3`、`OFF_SEQ=4`、`OFF_LEN=6`、`HEADER_LEN=8`）
- [x] 1.3 引用 `use crate::board_profile::BoardProfile;`（仅取 `MAX_GROUP_SIZE` 用于文档注释，不做运行时校验）

## 2. PacketHeader 与 PacketType

- [x] 2.1 定义 `#[repr(u8)] pub enum PacketType { Voice=0x01, Heartbeat=0x02, TalkState=0x03, CtrlBusyReady=0x04, PairBeaconHost=0x10, PairJoinReq=0x11, PairJoinAck=0x12, DirectoryBroadcast=0x20, ChannelSwitchAck=0x30 }`，派生 `Debug, Clone, Copy, PartialEq, Eq`
- [x] 2.2 实现 `PacketType::from_u8(u8) -> Result<Self, PacketError>`
- [x] 2.3 定义 `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct PacketHeader { pub ver: u8, pub ptype: PacketType, pub flags: u8, pub seq: u16, pub len: u16 }`（magic 固定不存储为字段，encode 时写入 0xC6）
- [x] 2.4 实现 `PacketHeader::parse(&[u8]) -> Result<(Self, &[u8]), PacketError>`：校验长度≥8、magic==0xC6（**不**校验 ver，D19：返回含 ver 的 header 供调用方检查），返回 header 与 payload 视图
- [x] 2.5 实现 `PacketHeader::encode(&self, &mut [u8]) -> Result<(), PacketError>`：写入 8 字节，校验 `self.len` 与 buf 剩余空间
- [x] 2.6 定义 `#[derive(Debug, Clone, PartialEq, Eq)] pub enum PacketError { BadMagic, BadVersion, Truncated, BadType, PayloadTooLarge }`

## 3. VOICE 包

- [x] 3.1 定义 `pub struct VoicePayload<'a> { pub sender_id: u8, pub effect: u8, pub opus_payload: &'a [u8] }`
- [x] 3.2 实现 `encode_voice(&PacketHeader, &VoicePayload, &mut [u8]) -> Result<usize, PacketError>`：按偏移 8/9/10 写入，header.len = 2 + opus_payload.len()
- [x] 3.3 实现 `decode_voice(&[u8]) -> Result<(PacketHeader, VoicePayload<'_>), PacketError>`：parse header 后校验 payload 长度 ≥ 2，opus_payload 指向 buf 视图

## 4. HEARTBEAT 包

- [x] 4.1 定义 `#[derive(Debug, Clone, Copy)] pub struct HeartbeatPayload { pub sender_id: u8, pub state: u8, pub mode: u8 }`
- [x] 4.2 实现 `encode_heartbeat` / `decode_heartbeat`，payload 长度固定 3

## 5. TALK_STATE 包

- [x] 5.1 定义 `#[derive(Debug, Clone, Copy)] pub struct TalkStatePayload { pub sender_id: u8, pub action: u8 }`
- [x] 5.2 实现 `encode_talk_state` / `decode_talk_state`，payload 长度固定 2

## 6. CTRL_BUSY_READY 包

- [x] 6.1 定义 `#[derive(Debug, Clone, Copy)] pub struct CtrlBusyReadyPayload { pub dst_id: u8, pub code: u8 }`
- [x] 6.2 实现 `encode_ctrl_busy_ready` / `decode_ctrl_busy_ready`，payload 长度固定 2

## 7. PAIR_BEACON_HOST 包

- [x] 7.1 定义 `#[derive(Debug, Clone, Copy)] pub struct PairBeaconHostPayload { pub host_mac: [u8;6], pub host_pub_key: [u8;32], pub mode: u8, pub cur_members: u8, pub max_members: u8, pub joinable: u8 }`
- [x] 7.2 实现 `encode_pair_beacon_host` / `decode_pair_beacon_host`，payload 长度固定 42（D15：6+32+1+1+1+1=42，非 50；50 为总包 8+42）

## 8. PAIR_JOIN_REQ 包

- [x] 8.1 定义 `#[derive(Debug, Clone, Copy)] pub struct PairJoinReqPayload { pub join_mac: [u8;6], pub join_pub_key: [u8;32], pub host_mac: [u8;6] }`
- [x] 8.2 实现 `encode_pair_join_req` / `decode_pair_join_req`，payload 长度固定 44

## 9. PAIR_JOIN_ACK 包

- [x] 9.1 定义 `#[derive(Debug, Clone, Copy)] pub struct PairJoinAckPayload { pub host_mac: [u8;6], pub host_pub_key: [u8;32], pub join_mac: [u8;6], pub accepted: u8, pub reason: u8 }`
- [x] 9.2 实现 `encode_pair_join_ack` / `decode_pair_join_ack`，payload 长度固定 46

## 10. DIRECTORY_BROADCAST 包

- [x] 10.1 定义 `pub struct DirectoryBroadcastPayload<'a> { pub member_count: u8, pub mode: u8, pub target_channel: u8, pub switch_offset: u16, entries: &'a [u8] }`，entries 长度 = member_count × 38（D9：switch_ts u32 → switch_offset u16）
- [x] 10.2 定义 `pub struct DirectoryEntry<'a> { pub mac: &'a [u8;6], pub pub_key: &'a [u8;32] }` 并实现 `DirectoryBroadcastPayload::entry_at(&self, i: usize) -> Result<DirectoryEntry, PacketError>`，越界返回 `Truncated`（D50：命名结构体而非裸元组）
- [x] 10.3 实现 `encode_directory_broadcast`：写入偏移 8/9/10/11-12(BE u16)/13+，header.len = 5 + member_count×38（D9：payload 5+N×38，4 成员 = 5+152=157 payload / 165 总包）
- [x] 10.4 实现 `decode_directory_broadcast`：parse header 后按 member_count 切 entries 视图

## 11. CHANNEL_SWITCH_ACK 包

- [x] 11.1 定义 `#[derive(Debug, Clone, Copy)] pub struct ChannelSwitchAckPayload { pub sender_id: u8, pub status: u8 }`
- [x] 11.2 实现 `encode_channel_switch_ack` / `decode_channel_switch_ack`，payload 长度固定 2

## 12. state.rs 枚举

- [x] 12.1 新增 `src/intercom/state.rs`，定义 `#[derive(Debug, Clone, PartialEq)] pub enum IntercomState { Idle, Hosting(HostPhase), Joining(JoinPhase), Grouped(VoiceState) }`
- [x] 12.2 定义 `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum HostPhase { Discovering, CollectingPeers, Frozen, SwitchingChannel }`
- [x] 12.3 定义 `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum JoinPhase { Searching, Requesting, WaitingConfirm, SwitchingChannel }`
- [x] 12.4 定义 `#[repr(u8)] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum VoiceState { Idle, Talking, Listening, ChannelBusy }`
- [x] 12.5 定义 `#[repr(u8)] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum IntercomMode { Clear, Free }`
- [x] 12.6 定义 `#[repr(u8)] #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum VoiceEffect { Normal=0, PitchUp=1, PitchDown=2 }`，实现 `to_u8(&self) -> u8` 与 `from_u8(u8) -> Option<VoiceEffect>`（D30：供 change 14 复用）
- [x] 12.7 定义 `#[derive(Debug, Clone, PartialEq)] pub enum IntercomEvent { StateChanged(IntercomState), PeerOnline(u8, u8), PeerOffline(u8), VoiceActive(u8), ChannelBusy, PttReady, PairFailed(String) }`
- [x] 12.8 定义 `#[derive(Debug, Clone, PartialEq, Eq)] pub enum IntercomError { Busy, NotGrouped, ChannelBusy, InvalidState, Net(NetError) }`
- [x] 12.9 顶部 `use crate::services::network::NetError;`；若 change 06 未落地，加 `// TODO: remove once change 06 lands` 注释并暂用 `pub type NetError = u8;` 占位（仅本模块可见，外部仍 `use` 正式路径）

## 13. 单元测试

- [x] 13.1 `#[cfg(test)] mod tests`：测试 `PacketHeader::parse` 的 magic/截断 两种错误路径（D19：不在 parse 内校验 ver，ver 路径由调用方测试）
- [x] 13.2 测试 `PacketType::from_u8` 已知值与未知值
- [x] 13.3 为 9 种包类型各写一个 encode→decode 往返测试，断言字段逐值一致
- [x] 13.4 测试 `DirectoryBroadcastPayload::entry_at` 越界返回错误
- [x] 13.5 测试 VOICE 的 `opus_payload` 视图指针落在原 buf 区间内
- [x] 13.6 测试 `switch_offset` 大端字节序 `[0x12,0x34]`（D9：u16 偏移，非 u32 时间戳）
- [x] 13.7 测试 `VoiceEffect::to_u8` / `from_u8` 往返：Normal=0, PitchUp=1, PitchDown=2, `from_u8(9)==None`（D30）

## 14. 构建验证

- [x] 14.1 执行 `cargo build`，确认零编译错误
- [x] 14.2 执行 `cargo test --lib intercom`，确认全部测试通过
- [x] 14.3 执行 `cargo build --release`，确认 release profile 通过
- [x] 14.4 确认 `src/main.rs` 未引用新模块（本期纯数据层，main 不变）

## 15. 收尾

- [x] 15.1 提交 commit：`feat: intercom packet format + state enums (change 08/17)`
- [x] 15.2 在 commit message 注明后续 change 09-13 将引用 `intercom::packet::*` 与 `intercom::state::*`


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
