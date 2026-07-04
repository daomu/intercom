## 1. 模块骨架与状态机定义

- [ ] 1.1 复用 change 08 的 `IntercomState` 枚举（不再定义独立 `PairingState`），在 `src/intercom/pairing.rs` 中使用 `IntercomState::Idle` / `Hosting(HostPhase)` / `Joining(JoinPhase)` / `Grouped(VoiceState)`；`HostPhase` 变体：`Discovering` / `CollectingPeers { members: Vec<PeerEntry> }` / `Frozen { members: Vec<PeerEntry>, target: u8, offset: u16 }` / `SwitchingChannel { target: u8, deadline: Instant }`
- [ ] 1.2 定义 `JoinPhase` 变体（`Searching { hosts }` / `Requesting { host }` / `WaitingConfirm { host }` / `SwitchingChannel { target, deadline }`）；未组网用 `IntercomState::Idle`（NOT `Ungrouped`）
- [ ] 1.3 定义 `PairingFailure` enum（`SearchTimeout / HostEnded / GroupFull / SchemaIncompatible / SignalWeak / StateChanged`）
- [ ] 1.4 定义 `HostInfo` struct（name / mac_suffix4 / rssi_4bar / cur_members / max_members / mode / joinable / last_seen_ms）
- [ ] 1.5 定义 `GroupInfo` struct（my_priv / peers: Vec<(Mac,PubKey)> / channel / mode）供 save_group 调用
- [ ] 1.6 定义 `PairingHandle`（`Arc<Mutex<IntercomState>>` + 失败码通道）供 UI 层读取
- [ ] 1.7 在 `src/intercom/mod.rs` 注册 `pub mod pairing;`

## 2. 阶段1 Host 侧逻辑

- [ ] 2.1 实现 `host_create(mode)`：`network.init(1)` + `crypto.gen_keypair()` + 状态转移到 `Hosting(Discovering)` → `Hosting(CollectingPeers)` + 启动 200ms beacon 定时器
- [ ] 2.2 实现 `broadcast_beacon()`：`encode_pair_beacon_host(...)`（host_mac + host_pub + mode + cur_members + max=4 + joinable）并 `network.send_broadcast(payload)`
- [ ] 2.3 实现 `on_recv_join_req(payload)`：`parse_pair_join_req(...)` + 校验包头 `ver == SCHEMA_VER`（不匹配 → `accepted=0, reason=2=SchemaIncompatible`）+ 校验成员数 < 4（不满足 → `reason=1=Full`）+ 校验 join_pub_key 合法性（非全 0/全 0xFF/长度 32，不满足 → `reason=2`）→ 追加 members + 单播 `PAIR_JOIN_ACK(accepted=1, reason=0=Accepted)` + `derive_lmk + add_peer`；不通过则 `accepted=0` + 对应 reason 码（1=Full / 2=SchemaIncompatible / 3=StateChanged）
- [ ] 2.4 实现 `encode_pair_join_ack(host_mac, host_pub, join_mac, accepted, reason)` 调用（由 change 08 提供 encode 函数，本期仅调用）
- [ ] 2.5 启动 90s 组队超时定时器：超时 → `host_cancel()` → 清理 members + clear_peers + 回 `IntercomState::Idle` + 暴露超时提示

## 3. 阶段1 Join 侧逻辑

- [ ] 3.1 实现 `join_search_start()`：`network.init(1)` + 注册 recv 回调 + 状态转移到 `Joining(Searching)`
- [ ] 3.2 实现 `on_recv_beacon(payload)`：`parse_pair_beacon_host(...)` → 按 host_mac 去重更新 `hosts` 缓存（含 RSSI 平滑+滞回映射 4 格 + last_seen_ms）
- [ ] 3.3 实现 hosts 缓存过期：100ms tick 定时器检查 `last_seen_ms` 超 5s 的条目标记 stale 移出可加入列表
- [ ] 3.4 实现 `join_request(host_mac)`：`crypto.gen_keypair()` + `encode_pair_join_req(...)` + `network.send_unicast(host_mac, payload)` + 状态转移到 `Joining(Requesting)` + 启动 3s 请求超时
- [ ] 3.5 实现 `on_recv_join_ack(payload)`：`parse_pair_join_ack(...)` → accepted=1, reason=0 → `derive_lmk(join_priv, host_pub) + add_peer(host_mac, lmk)` + 状态 `Joining(WaitingConfirm)`；accepted=0 → 按 reason 码（1=Full / 2=SchemaIncompatible / 3=StateChanged）暴露 `PairingFailure` + 回 `Joining(Searching)`
- [ ] 3.6 实现 10s 搜索超时：未收到任何 beacon → 暴露 `PairingFailure::SearchTimeout`（保持 `Joining(Searching)`，提供重试）

## 4. 阶段2 Host 侧逻辑

- [ ] 4.1 实现 `host_confirm()`：状态 `Hosting(CollectingPeers) → Hosting(Frozen)`，冻结 members 列表（冻结点）
- [ ] 4.2 调用 `network.evaluate_channels([1,6,11,13])` 选定 target_channel
- [ ] 4.3 计算 `switch_offset: u16`（相对 ms），调度 `schedule_at(now_host + switch_offset, switch_channel)`
- [ ] 4.4 经定时器队列调度 5 次 `DIRECTORY_BROADCAST` 广播：`encode_directory_broadcast(...)`（member_count + mode + target_channel + switch_offset 大端 + 成员条目数组 38B/项）+ `send_broadcast(payload)`，调度时刻 `now + 0, 80, 160, 240, 320 ms`（非 `sleep(80ms) × 5` 阻塞，Task B 期间可处理 incoming ACK / PAIR_JOIN_REQ）
- [ ] 4.5 设置 `radio_busy = true` 标志（若未在 host_create 时设置）

## 5. 阶段2 Join 侧逻辑

- [ ] 5.1 实现 `on_recv_directory(payload)`：`parse_directory_broadcast(...)` → 缓存 directory（peers + target_channel + switch_offset）+ 按 `now_join + switch_offset` 调度 `schedule_at(switch_at, switch_channel)` + 状态 `Joining(WaitingConfirm) → Joining(SwitchingChannel)`；后续冗余 DIRECTORY_BROADCAST 忽略
- [ ] 5.2 实现 DIRECTORY_BROADCAST 5s 超时：`Joining(WaitingConfirm)` 状态超 5s 未收到 → 暴露 `PairingFailure::HostEnded` + 回 `IntercomState::Idle`

## 6. 阶段3 双侧切换逻辑

- [ ] 6.1 实现 `switch_channel(target)`（Host 侧）：`network.clear_peers()` → `network.set_channel(target)` → for each peer in members (except self): `derive_lmk + add_peer`
- [ ] 6.2 实现 `switch_channel(target)`（Join 侧）：同 6.1 顺序，peers 来源为 directory.peers（除自己）
- [ ] 6.3 Join 侧重构后 `encode_channel_switch_ack(...)` + `send_unicast(host_mac, payload)` 发送 `CHANNEL_SWITCH_ACK(sender_id, status=0)` 给 Host（加密单播）
- [ ] 6.4 Host 侧实现 ACK 收集：`Hosting(SwitchingChannel)` 状态等待 ACK，收齐全部 members ACK 或 2s 超时 → 进入 `Grouped(Idle)`；超时未 ACK 的成员标"待确认/离线"但不阻塞
- [ ] 6.5 双侧在进入 `Grouped(Idle)` 前调用 `storage.save_group(GroupInfo{my_priv, peers, channel, mode})`
- [ ] 6.6 进入 `Grouped(Idle)` 后释放 `radio_busy = false`

## 7. 用户取消与资源清理

- [ ] 7.1 实现 `host_cancel()`：停止 beacon 定时器 + clear_peers + 状态回 `IntercomState::Idle` + 释放 radio_busy
- [ ] 7.2 实现 `join_cancel()`：清 hosts 缓存 + clear_peers（若已建立初步 LMK）+ 状态回 `IntercomState::Idle` + 释放 radio_busy
- [ ] 7.3 取消仅允许在 `Hosting(Discovering)` / `Hosting(CollectingPeers)` 状态（Join 侧：`Joining(Searching)` / `Joining(Requesting)` / `Joining(WaitingConfirm)`）；`Hosting(Frozen)` 及之后状态不可取消（已下发 switch_offset，须走完阶段3 或等超时）

## 8. 共享状态与 UI 数据契约

- [ ] 8.1 实现 `PairingHandle` 读接口：`get_host_list() -> Vec<HostInfo>` / `get_current_state() -> IntercomState` / `get_failure() -> Option<PairingFailure>`
- [ ] 8.2 实现 RSSI → 4 格映射函数（含平滑+滞回，遵循技术设计 §13.2）
- [ ] 8.3 实现 MAC 后 4 位格式化（`format_mac_suffix4(mac) -> String`）
- [ ] 8.4 默认主机名称生成：`"Host-" + mac_suffix4`（设备名称未由 change 07 设置时用此默认值）

## 9. 单元测试

- [ ] 9.1 测试 `derive_lmk` 对称性：A→B LMK == B→A LMK（调用 change 03 CryptoService）
- [ ] 9.2 测试 `PairingFailure` enum 全变体可构造
- [ ] 9.3 测试 `HostInfo` 字段完整性（6 字段非空）
- [ ] 9.4 测试 RSSI → 4 格映射的滞回边界（-85dBm → 1 格/0 格切换不抖动）
- [ ] 9.5 测试 DIRECTORY_BROADCAST payload 编解码往返（`encode_directory_broadcast` / `parse_directory_broadcast`，member_count + switch_offset 大端 + N×38 条目）

## 10. 编译验证

- [ ] 10.1 `cargo build` 在 `riscv32imc-esp-espidf` target 下零错误通过
- [ ] 10.2 `cargo build --release` 通过
- [ ] 10.3 `cargo clippy` 无 warning（除第三方 crate）

## 11. 双机验收

- [ ] 11.1 烧录 2 台 Waveshare ESP32-C6-Touch-LCD-1.54
- [ ] 11.2 设备 A 创建主机（清晰模式）→ 进入 `Hosting(CollectingPeers)` → 串口确认 beacon 200ms 周期广播
- [ ] 11.3 设备 B 搜索主机 → 列表出现 A（名称 + MAC 后 4 位 + 信号 4 格 + 0/4 人数 + 清晰 + joinable=1）
- [ ] 11.4 设备 B 选择 A → 发 PAIR_JOIN_REQ → 收到 PAIR_JOIN_ACK(accepted=1, reason=0) → `Joining(WaitingConfirm)`
- [ ] 11.5 设备 A 串口确认 members 含 B，初步 LMK 已 add_peer
- [ ] 11.6 设备 A 点击确认 → `Hosting(Frozen)` → 串口确认定时器队列调度 5 次 DIRECTORY_BROADCAST（0/80/160/240/320ms）+ target_channel + switch_offset
- [ ] 11.7 到 `now_join + switch_offset` 后双侧切信道 → Join 发 CHANNEL_SWITCH_ACK → 双侧进入 `Grouped(Idle)`
- [ ] 11.8 重启设备 A/B → 串口确认 NVS group 信息已写入（load_group 不报错，恢复逻辑由 change 12 实现）
- [ ] 11.9 验证组队超时：A 创建主机后不确认，等 90s → 自动回 `IntercomState::Idle`，UI 提示"组队超时，已结束"
- [ ] 11.10 验证失败场景：B 搜索无主机 10s → 提示"搜索超时"；A 满员后 B 加入 → 收到 `reason=1=Full` 提示"人数已满"

## 12. 收尾

- [ ] 12.1 提交 commit：`feat: implement three-phase ECDH pairing state machine (change 09/17)`
- [ ] 12.2 在 commit message 注明依赖 change 03/06/08（deps = `03, 06, 08`），引用技术设计 §5/§19.1/§19.2 与 PRD §11
