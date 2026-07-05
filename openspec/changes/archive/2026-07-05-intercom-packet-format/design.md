## Context

仓库当前状态：change 01 已落地工程骨架，`src/intercom/mod.rs` 为空占位（`#![allow(dead_code)]`）。技术设计 §4 与 §5 已完整定义 9 种 ESP-NOW 包的字节级布局，§3.5 已定义全部对讲业务枚举签名。本期目标：把这些文档定义固化为可编译、可测试的 Rust 数据层，使后续 change 09-13 在业务层只关心状态迁移与网络收发，不再处理字节级序列化。

关键约束：
- ESP32-C6 无 PSRAM、512KB SRAM，实时音频路径禁止高频动态分配（PRD §16.9）
- ESP-NOW 单包上限约 250B（技术设计 §4.2 备注）
- magic=0xC6 用于空中过滤非本协议包；seq 大端 u16 用于语音去重（PRD §16.4：收到序号 ≤ 当前已处理最大序号 → 直接丢弃）
- 阶段 1/2 包明文广播/单播，阶段 3 起加密单播（PRD §18.4、§18.5）——但加密由 ESP-NOW 硬件 AES-CCM 在 NetworkService 层处理，packet.rs 只产出明文 payload 字节

## Goals / Non-Goals

**Goals:**
- `packet.rs` 提供 9 种包类型的 `encode_*` / `decode_*` 函数，输入输出均为 `&[u8]` / `&mut [u8]`，零堆分配友好
- `PacketHeader` 8 字节固定布局，magic/ver/seq/len 字段严格按技术设计 §4.1 大端编码
- `state.rs` 定义技术设计 §3.5 全部枚举，签名与文档一致（含 `IntercomError::Net(NetError)` 变体）
- `PacketError` 枚举覆盖 magic/ver/type/长度四类错误，decode 失败时返回错误而非 panic
- 单元测试覆盖每种包的 encode→decode 往返与错误路径
- `cargo build` 在 `riscv32imc-esp-espidf` target 零错误通过

**Non-Goals:**
- 实际网络收发（ESP-NOW send/recv）→ change 06
- ECDH/LMK 计算、加密表注册 → change 03/09
- 状态机迁移逻辑（Idle→Hosting→Grouped 等）→ change 09/12
- 心跳超时、语音去重缓冲、jitter buffer → change 10/11/12
- 任何 task 创建或并发原语 → 后续变更
- 序号去重算法本身（本期仅提供 seq 字段的编解码，去重逻辑在 change 10/11）

## Decisions

### D1：序列化 = 手写字节级 encode/decode，不引入 serde
实时音频路径禁止高频动态分配（PRD §16.9）。`serde` + `serde_bytes` 虽方便但引入反射式分配与未使用依赖体积。手写 `buf[0..8].copy_from_slice(...)` 零分配、编译期可预测、对 ESP-NOW 单包上限友好。备选：`heapless::Bytes`——仍属额外依赖，且本项目的字节布局已完全固定，无需泛型序列化框架，排除。

### D2：Payload 结构体持引用而非拥有，避免堆分配
`VoicePayload<'a>` 的 `opus_payload: &'a [u8]` 在 decode 时直接指向调用方提供的 buf 切片，encode 时从结构体引用写入目标 buf。其余小 payload（≤48B）用 `Copy` 字段直接持有。`DirectoryBroadcastPayload` 的 entries 用 `&'a [u8]` 视图 + 提供迭代器/`entry_at(i)` 访问，避免在栈上放大数组。备选：`heapless::Vec`——entries 数量上限 4，38×4=152B 栈可承受，但 `&[u8]` 视图更省且零拷贝，排除 owning 方案。

### D3：magic 校验在 `PacketHeader::parse` 完成，ver 延迟到调用方
`parse(buf)` 首先校验 `buf.len() >= 8`、`buf[0] == 0xC6`，违反即返回 `PacketError::BadMagic` / `Truncated`。`parse` 返回含 `ver` 字段的 header 供调用方检查，**不**在 parse 内拒绝 ver 不符的包。`SCHEMA_VER` 常量与 NVS schema 版本同步（技术设计 §4.1），本期固定为 `1`。ver 校验由 pairing 层（change 09）在收到 PAIR_JOIN_REQ 后检查 `ver != SCHEMA_VER` → 回 PAIR_JOIN_ACK(accepted=0, reason=2)；非 pairing 层调用方检查 ver 后静默丢弃不符包。备选：在 parse 内统一拒绝 ver——但 pairing 层需要根据 ver 不符回拒绝 ACK 而非静默丢弃，故 parse 不拒绝，由调用方按场景处理，排除统一拒绝方案。

### D4：type 字段 = `PacketType` 枚举，未知值返回 `BadType`
`PacketType::from_u8(u8) -> Result<Self, PacketError>`。新增包类型时只需在枚举加变体 + match arm。`flags` 字段本期保留不解析（技术设计 §4.1 标注"保留位"），仅原样读写。

### D5：seq/len/switch_offset 大端编码
技术设计 §4.1 明确 seq/len 为大端；§5.5 switch_offset 为大端 u16（D9：原 switch_ts u32 已替换为相对偏移 u16，节省 2 字节并消除时钟域不一致）。Rust 默认 little-endian，必须显式 `u16::from_be_bytes` / `u16::to_be_bytes`。备选：小端——违反文档字节序约定，且抓包分析困难，排除。

### D6：state.rs 枚举签名与技术设计 §3.5 1:1 对齐
`IntercomState::Grouped(VoiceState)`、`IntercomError::Net(NetError)` 等变体保持与文档签名完全一致，便于后续 change 直接复制 trait 签名。`NetError` 通过 `use crate::services::network::NetError`（change 06 将定义）引用；本期在 `state.rs` 顶部 `use crate::services::network::NetError;`，若 change 06 未完成则用 `#[allow(unused)]` 占位编译通过。备选：本期自定义 `NetError`——与 change 06 重复定义，引发类型冲突，排除。

### D7：DIRECTORY_BROADCAST entries 视图 + `entry_at` 访问器
成员条目 38B/项，N ≤ 4。`DirectoryBroadcastPayload<'a>` 持 `entries: &'a [u8]`（长度 = member_count×38），提供 `fn entry_at(&self, i: usize) -> Result<DirectoryEntry, PacketError>` 返回命名结构体 `DirectoryEntry { mac: &[u8;6], pub_key: &[u8;32] }` 视图（D50：命名结构体而非裸元组）。避免在 decode 时复制到栈数组。备选：`heapless::Vec<DirectoryEntry, 4>`——拷贝开销小但 API 复杂化，且本期目标是数据层最薄，排除。

### D8：测试 = host 单元测试（`#[cfg(test)]`），不上机
包编解码是纯字节操作，host `cargo test` 即可覆盖。无需 on-device 验收。备选：on-device 往返——网络层未就绪，无法验证，排除。

## Risks / Trade-offs

- **[NetError 类型在 change 06 才落地]** → `state.rs` 用 `use crate::services::network::NetError;`；若 change 06 未合入，本期 `cargo build` 会失败。**缓解**：本期 `src/services/network/mod.rs` 若仅有空 trait 签名（change 06 未完成），则在 `state.rs` 顶部条件引用或暂以 `pub type NetError = u8;` 占位注释标注 TODO，待 change 06 落地后移除占位
- **[ESP-NOW 单包上限约 250B，DIRECTORY_BROADCAST 在 4 成员时 = 8 header + 5 payload-header + 4×38 entries = 165B（D9 2 字节 offset 后），安全；但 opus_payload 需调用方控制]** → `encode_voice` 不强制校验 opus_payload 长度上限（上层 AudioService 在 change 05 配置 Opus 使单帧 ≤240B），仅在 `PacketHeader::encode` 校验 `len` 字段 ≤ `u16::MAX`。**缓解**：`PacketError::PayloadTooLarge` 在 `len` 超过 `u16::MAX` 时返回（实际不会触发，留作防御）
- **[手写字节序容易 off-by-one]** → 单元测试覆盖每种包每个字段的偏移；用常量 `const OFF_SEQ: usize = 4;` 等命名偏移，避免裸数字
- **[schema 版本演进时旧设备包被拒]** → `SCHEMA_VER` 为常量，升级时全网需同步；本期版本=1，后续演进通过 change 显式升级
- **[flags 字段未定义语义]** → 本期原样读写不解析，后续若需复用（如重传标记）在后续 change 加语义

## Migration Plan

无既有运行时需迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译（含 `state.rs` 对 `NetError` 的引用解析）
3. `cargo test --lib intercom::packet` 验证 9 种包往返与错误路径
4. 后续 change 09 在此基础上实现组队状态机

回滚：`git revert <commit>`，`src/intercom/mod.rs` 回到空占位。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。`NetError` 引用策略见 Risks 缓解项。
