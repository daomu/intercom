## ADDED Requirements

### Requirement: 通用包头布局
`src/intercom/packet.rs` SHALL 定义 `PacketHeader` 为 8 字节固定布局，字段顺序与偏移严格遵循技术设计 §4.1：偏移 0 = magic（固定 0xC6）、偏移 1 = ver（schema 版本）、偏移 2 = type（包类型）、偏移 3 = flags（保留位）、偏移 4-5 = seq（u16 大端）、偏移 6-7 = len（u16 大端，payload 字节数）。`PacketHeader::parse` SHALL 仅校验 `magic == 0xC6`（及缓冲长度 ≥ 8），返回含 `ver` 字段的 header 供调用方检查；parse **不**在 ver 不符时拒绝（D19：ver 校验由 pairing 层 change 09 与非 pairing 层调用方负责）。

#### Scenario: encode 后字节序符合大端约定
- **WHEN** 构造 `PacketHeader { magic: 0xC6, ver: 1, type: Voice, flags: 0, seq: 0x1234, len: 0x00FF }` 并调用 `encode(&mut buf)`
- **THEN** `buf[0]==0xC6`、`buf[1]==1`、`buf[2]==0x01`、`buf[3]==0`、`buf[4]==0x12`、`buf[5]==0x34`、`buf[6]==0x00`、`buf[7]==0xFF`

#### Scenario: parse 校验 magic 与 ver
- **WHEN** 调用 `PacketHeader::parse(&[0x00, ...])`（首字节非 0xC6）
- **THEN** 返回 `Err(PacketError::BadMagic)`
- **WHEN** 调用 `PacketHeader::parse(&[0xC6, 0x99, ...])`（ver 与 `SCHEMA_VER` 不符）
- **THEN** 返回 `Ok(header)` 且 `header.ver == 0x99`（parse 仅校验 magic，ver 由调用方检查；pairing 层 change 09 处理 ver 不符，非 pairing 层静默丢弃）

#### Scenario: parse 拒绝截断缓冲
- **WHEN** 调用 `PacketHeader::parse` 传入长度 < 8 的切片
- **THEN** 返回 `Err(PacketError::Truncated)`

### Requirement: 包类型枚举
`PacketType` SHALL 为 `#[repr(u8)]` 枚举，变体取值严格遵循技术设计 §4.1 类型表：`Voice=0x01`、`Heartbeat=0x02`、`TalkState=0x03`、`CtrlBusyReady=0x04`、`PairBeaconHost=0x10`、`PairJoinReq=0x11`、`PairJoinAck=0x12`、`DirectoryBroadcast=0x20`、`ChannelSwitchAck=0x30`。

#### Scenario: from_u8 解析已知类型
- **WHEN** 调用 `PacketType::from_u8(0x10)`
- **THEN** 返回 `Ok(PacketType::PairBeaconHost)`

#### Scenario: from_u8 拒绝未知类型
- **WHEN** 调用 `PacketType::from_u8(0xFF)`
- **THEN** 返回 `Err(PacketError::BadType)`

### Requirement: VOICE 包编解码
`VoicePayload` SHALL 按 §4.2 布局编解码：偏移 8 = sender_id（u8）、偏移 9 = effect（u8，对齐 `VoiceEffect` 数值）、偏移 10 起 = opus_payload 变长切片。`encode_voice` SHALL 将 header + payload 写入调用方提供的 `&mut [u8]`，并在 `len` 字段记录 payload 字节数。`decode_voice` SHALL 返回 `VoicePayload<'a>` 持 `opus_payload: &'a [u8]` 视图，零堆分配。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造 `VoicePayload { sender_id: 2, effect: 1, opus_payload: &[0xDE, 0xAD] }` 并 encode 后 decode
- **THEN** 解出的 `sender_id==2`、`effect==1`、`opus_payload==[0xDE, 0xAD]`

#### Scenario: header.len 与 payload 一致
- **WHEN** 对上述 payload 执行 encode
- **THEN** 写入的 header `len` 字段 == 2 + 2（sender_id+effect+opus 长度）

### Requirement: HEARTBEAT 包编解码
`HeartbeatPayload` SHALL 按 §4.3 布局：偏移 8 = sender_id、偏移 9 = state（对应 `VoiceState` 数值）、偏移 10 = mode（对应 `IntercomMode` 数值）。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造 `HeartbeatPayload { sender_id: 1, state: VoiceState::Talking, mode: IntercomMode::Clear }` 并 encode 后 decode
- **THEN** 解出字段与原值一致

### Requirement: TALK_STATE 包编解码
`TalkStatePayload` SHALL 按 §4.4 布局：偏移 8 = sender_id、偏移 9 = action（1=开始发言，0=结束发言）。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造 `TalkStatePayload { sender_id: 3, action: 1 }` 并 encode 后 decode
- **THEN** 解出 `sender_id==3`、`action==1`

### Requirement: CTRL_BUSY_READY 包编解码
`CtrlBusyReadyPayload` SHALL 按 §4.5 布局：偏移 8 = dst_id、偏移 9 = code（1=CHANNEL_BUSY，2=PTT_READY）。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造 `CtrlBusyReadyPayload { dst_id: 0, code: 2 }` 并 encode 后 decode
- **THEN** 解出 `dst_id==0`、`code==2`

### Requirement: PAIR_BEACON_HOST 包编解码
`PairBeaconHostPayload` SHALL 按 §5.2 布局：偏移 8 = host_mac(6B)、偏移 14 = host_pub_key(32B)、偏移 46 = mode(1B)、偏移 47 = cur_members(1B)、偏移 48 = max_members(1B)、偏移 49 = joinable(1B)，总 payload 长度 42 字节（6+32+1+1+1+1，不含 8 字节 header；总包 50 字节 = 8 header + 42 payload）。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造完整 `PairBeaconHostPayload` 并 encode 后 decode
- **THEN** 解出的 host_mac、host_pub_key、mode、cur_members、max_members、joinable 与原值逐字节一致

#### Scenario: max_members 与 BoardProfile 一致
- **WHEN** 调用方构造该包时传入 `max_members`
- **THEN** 该值 SHALL 不超过 `BoardProfile::MAX_GROUP_SIZE`（4），由调用方保证；packet 层不强制

### Requirement: PAIR_JOIN_REQ 包编解码
`PairJoinReqPayload` SHALL 按 §5.3 布局：偏移 8 = join_mac(6B)、偏移 14 = join_pub_key(32B)、偏移 46 = host_mac(6B)，总 payload 长度 44 字节。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造完整 `PairJoinReqPayload` 并 encode 后 decode
- **THEN** 解出 join_mac、join_pub_key、host_mac 与原值一致

### Requirement: PAIR_JOIN_ACK 包编解码
`PairJoinAckPayload` SHALL 按 §5.4 布局：偏移 8 = host_mac(6B)、偏移 14 = host_pub_key(32B)、偏移 46 = join_mac(6B)、偏移 52 = accepted(1B)、偏移 53 = reason(1B)，总 payload 长度 46 字节。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造完整 `PairJoinAckPayload` 并 encode 后 decode
- **THEN** 解出全部字段与原值一致

### Requirement: DIRECTORY_BROADCAST 包编解码
`DirectoryBroadcastPayload` SHALL 按 §5.5 布局：偏移 8 = member_count(1B)、偏移 9 = mode(1B)、偏移 10 = target_channel(1B)、偏移 11-12 = switch_offset(2B 大端 u16，相对接收时刻的毫秒偏移；D9：原 switch_ts u32 绝对时间已替换)、偏移 13 起 = entries（member_count × 38 字节，每项 = mac(6B)+pub_key(32B)）。`DirectoryBroadcastPayload<'a>` SHALL 持 `entries: &'a [u8]` 视图并提供 `entry_at(i)` 访问器返回 `DirectoryEntry { mac: &[u8;6], pub_key: &[u8;32] }` 命名结构体视图（D50：命名结构体而非裸元组）。总 payload = 5 + member_count×38（4 成员 = 5+152=157 payload，165 总包）。

#### Scenario: 4 成员目录往返一致
- **WHEN** 构造 `member_count=4`、含 4 组 mac+pub_key 的 payload 并 encode 后 decode
- **THEN** 解出 `member_count==4`、`entry_at(0..3)` 返回的 `DirectoryEntry` 的 mac 与 pub_key 与原值逐字节一致

#### Scenario: switch_offset 大端编码
- **WHEN** 构造 `switch_offset=0x1234` 并 encode
- **THEN** 写入字节偏移 11-12 为 `[0x12, 0x34]`

#### Scenario: entry_at 越界拒绝
- **WHEN** 对 `member_count=2` 的 payload 调用 `entry_at(3)`
- **THEN** 返回 `Err(PacketError::Truncated)` 或等价越界错误

### Requirement: CHANNEL_SWITCH_ACK 包编解码
`ChannelSwitchAckPayload` SHALL 按 §5.6 布局：偏移 8 = sender_id(1B)、偏移 9 = status(1B，0=到达成功，非0=失败码)。

#### Scenario: encode→decode 往返一致
- **WHEN** 构造 `ChannelSwitchAckPayload { sender_id: 1, status: 0 }` 并 encode 后 decode
- **THEN** 解出 `sender_id==1`、`status==0`

### Requirement: PacketError 错误覆盖
`PacketError` SHALL 为枚举，变体至少包含 `BadMagic`、`BadVersion`、`Truncated`、`BadType`、`PayloadTooLarge`。所有 decode 函数在对应校验失败时 SHALL 返回相应错误，绝不 panic。

#### Scenario: payload 长度不足返回 Truncated
- **WHEN** 调用 `decode_voice` 传入 buf 长度 < header + 2
- **THEN** 返回 `Err(PacketError::Truncated)`

#### Scenario: len 字段超 u16::MAX 返回 PayloadTooLarge
- **WHEN** 构造 payload 长度 > 65535 并调用 `PacketHeader::encode`
- **THEN** 返回 `Err(PacketError::PayloadTooLarge)`

### Requirement: 零堆分配友好
所有 `encode_*` 函数 SHALL 接受 `&mut [u8]` 输出缓冲；所有 `decode_*` 返回的 payload 结构体 SHALL 持 `&'a [u8]` 视图而非 owning 集合，确保实时音频路径无高频动态分配（PRD §16.9）。

#### Scenario: decode 返回的 opus_payload 指向调用方 buf
- **WHEN** 调用 `decode_voice(buf)` 并检查返回的 `opus_payload` 指针
- **THEN** `opus_payload.as_ptr()` 落在 `buf.as_ptr()` 到 `buf.as_ptr()+buf.len()` 区间内

### Requirement: 模块注册
`src/intercom/mod.rs` SHALL 通过 `pub mod packet;` 注册 packet 模块，使其可被 `crate::intercom::packet::*` 引用。

#### Scenario: 后续变更可引用
- **WHEN** change 09 在 `src/intercom/pairing.rs` 中 `use crate::intercom::packet::encode_pair_beacon_host`
- **THEN** 编译期路径解析成功，无 unresolved import 错误
