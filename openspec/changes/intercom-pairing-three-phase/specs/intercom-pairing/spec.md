## ADDED Requirements

### Requirement: Host 状态机
`pairing.rs` SHALL 复用 change 08 的 `IntercomState` 枚举实现 Host 侧状态机，状态集为 `{Idle, Hosting(HostPhase)}`，其中 `HostPhase` 变体为 `{Discovering, CollectingPeers{members}, Frozen{members,target,offset}, SwitchingChannel{target,deadline}}`，终态为 `Grouped(Idle)`（`VoiceState::Idle`）。转移路径 SHALL 为：`Idle → Hosting(Discovering) → Hosting(CollectingPeers) → Hosting(Frozen) → Hosting(SwitchingChannel) → Grouped(Idle)`；任一非终态在组队超时或用户取消时 SHALL 回退到 `Idle`。状态转移 SHALL 在 Task B（网络业务 task）单线程内同步执行，不引入 async runtime。

#### Scenario: 创建主机进入发现
- **WHEN** 用户在未组网页选择"创建主机"并选定模式（清晰/自由）
- **THEN** 状态从 `Idle` 转移到 `Hosting(Discovering)`，调用 `NetworkService::init(DISCOVERY_CHANNEL=1)`，生成 Curve25519 密钥对，随后立即进入 `Hosting(CollectingPeers)` 并开始周期广播 `PAIR_BEACON_HOST`

#### Scenario: 收到 Join 请求收集成员
- **WHEN** Host 处于 `Hosting(CollectingPeers)` 且收到 `PAIR_JOIN_REQ`
- **THEN** Host 先校验包头 `ver == SCHEMA_VER`（不匹配则发送 `PAIR_JOIN_ACK(accepted=0, reason=2=SchemaIncompatible)`，不追加成员），再校验成员数 < MAX_GROUP_SIZE(=4) 与公钥合法性（全 0 / 全 0xFF / 长度 ≠ 32 → reason=2），通过后追加到 members 列表，单播 `PAIR_JOIN_ACK(accepted=1, reason=0=Accepted)`，并对该 Join 调用 `derive_lmk + add_peer` 建立初步 LMK

#### Scenario: 拒绝已满成员
- **WHEN** Host 处于 `Hosting(CollectingPeers)` 且 members 已达 MAX_GROUP_SIZE(=4) 时收到 `PAIR_JOIN_REQ`
- **THEN** Host 单播 `PAIR_JOIN_ACK(accepted=0, reason=1=Full)`，不追加成员

#### Scenario: 冻结后拒绝新成员
- **WHEN** Host 处于 `Hosting(Frozen)` 或后续状态时收到 `PAIR_JOIN_REQ`
- **THEN** Host 单播 `PAIR_JOIN_ACK(accepted=0, reason=3=StateChanged)`，不追加成员

#### Scenario: 确认组队冻结成员
- **WHEN** Host 处于 `Hosting(CollectingPeers)` 时用户点击"确认组队"
- **THEN** 状态转移到 `Hosting(Frozen)`，members 列表立即冻结（冻结点 = host_confirm 调用），后续不再接受任何 `PAIR_JOIN_REQ`；立即执行阶段2 逻辑（评估信道 + 广播通讯录）

#### Scenario: 切信道完成进入已组网
- **WHEN** Host 处于 `Hosting(SwitchingChannel)` 且完成 `clear_peers → set_channel(target) → 全网状 add_peer`，且收到全部成员的 `CHANNEL_SWITCH_ACK` 或达到 2s 超时
- **THEN** 状态转移到 `Grouped(Idle)`，调用 `storage.save_group` 持久化组信息，释放 `radio_busy` 标志

#### Scenario: 组队超时自动取消
- **WHEN** Host 进入 `Hosting(CollectingPeers)` 后 90s（60~120s 区间取中值）未调用 `host_confirm`
- **THEN** 状态强制回退到 `Idle`，停止 beacon 广播，清理已建立的初步 LMK（clear_peers），UI 提示"组队超时，已结束"

#### Scenario: 用户主动取消
- **WHEN** 用户在 `Hosting(Discovering)` / `Hosting(CollectingPeers)` 状态选择"取消组队"
- **THEN** 状态强制回退到 `Idle`，清理资源（定时器、初步 LMK），不写 NVS。`Hosting(Frozen)` 及之后状态不可取消（须走完阶段3 或等超时）

### Requirement: Join 状态机
`pairing.rs` SHALL 复用 `IntercomState` 实现 Join 侧状态机，状态集为 `{Idle, Joining(JoinPhase)}`，其中 `JoinPhase` 变体为 `{Searching{hosts}, Requesting{host}, WaitingConfirm{host}, SwitchingChannel{target,deadline}}`，终态为 `Grouped(Idle)`。转移路径 SHALL 为：`Idle → Joining(Searching) → Joining(Requesting) → Joining(WaitingConfirm) → Joining(SwitchingChannel) → Grouped(Idle)`；任一非终态在失败或用户取消时 SHALL 回退到 `Idle` 并暴露失败原因码。

#### Scenario: 搜索主机
- **WHEN** 用户在未组网页选择"搜索并加入"
- **THEN** 状态转移到 `Joining(Searching)`，调用 `NetworkService::init(DISCOVERY_CHANNEL=1)`，注册 recv 回调监听 `PAIR_BEACON_HOST`

#### Scenario: 收到 beacon 更新主机列表
- **WHEN** Join 处于 `Joining(Searching)` 且收到 `PAIR_BEACON_HOST`
- **THEN** 按 host_mac 去重更新本地 hosts 缓存，HostInfo 字段含：名称、MAC 后 4 位、信号 4 格（RSSI 平滑+滞回）、当前人数/上限、模式、joinable 标志；5s 未再收到 beacon 的条目标记 stale 并移出可加入列表

#### Scenario: 选择主机发送请求
- **WHEN** 用户从主机列表选择一个 joinable=1 的主机
- **THEN** 状态转移到 `Joining(Requesting)`，生成 Curve25519 密钥对，单播 `PAIR_JOIN_REQ` 给目标 Host

#### Scenario: 收到接受响应
- **WHEN** Join 处于 `Joining(Requesting)` 且收到 `PAIR_JOIN_ACK(accepted=1, reason=0=Accepted)`
- **THEN** 对 Host 调用 `derive_lmk + add_peer` 建立初步 LMK，状态转移到 `Joining(WaitingConfirm)`

#### Scenario: 收到拒绝响应
- **WHEN** Join 处于 `Joining(Requesting)` 且收到 `PAIR_JOIN_ACK(accepted=0)`
- **THEN** 状态回退到 `Joining(Searching)`，UI 按 reason 码（1=Full / 2=SchemaIncompatible / 3=StateChanged）显示明确失败原因，提供重试入口

#### Scenario: 收到通讯录等待切换
- **WHEN** Join 处于 `Joining(WaitingConfirm)` 且收到首个 `DIRECTORY_BROADCAST`
- **THEN** 缓存 directory（成员表 + target_channel + switch_offset），按 `now_join + switch_offset` 调度 `schedule_at(switch_at, switch_channel)`，状态转移到 `Joining(SwitchingChannel)`；后续冗余 DIRECTORY_BROADCAST 忽略

#### Scenario: 切信道完成进入已组网
- **WHEN** Join 处于 `Joining(SwitchingChannel)` 且完成 `clear_peers → set_channel(target) → 全网状 add_peer`，且已发送 `CHANNEL_SWITCH_ACK(status=0)` 给 Host
- **THEN** 状态转移到 `Grouped(Idle)`，调用 `storage.save_group` 持久化组信息

#### Scenario: 搜索超时
- **WHEN** Join 处于 `Joining(Searching)` 且 10s 内未收到任何 `PAIR_BEACON_HOST`
- **THEN** 状态保持 `Joining(Searching)`，UI 提示"搜索超时，未发现主机"并提供重试入口（不强制回退，允许继续等待）

### Requirement: 阶段1 星型公钥收集
阶段1 SHALL 在 DISCOVERY_CHANNEL(=1) 完成 Host↔Join 的公钥交换与初步 LMK 建立。Host SHALL 每 200ms(±20ms) 广播一次 `PAIR_BEACON_HOST`（type=0x10，明文），payload 含 host_mac + host_pub_key + mode + cur_members + max_members + joinable。Join SHALL 单播 `PAIR_JOIN_REQ`（type=0x11，明文），payload 含 join_mac + join_pub_key + host_mac。Host SHALL 在发送 `PAIR_JOIN_ACK` 前校验包头 `ver` 字段：`ver != SCHEMA_VER` 时发送 `PAIR_JOIN_ACK(accepted=0, reason=2=SchemaIncompatible)`；非配对包 ver 不匹配时静默丢弃。Host SHALL 单播 `PAIR_JOIN_ACK`（type=0x12，明文），payload 含 host_mac + host_pub_key + join_mac + accepted + reason，reason 码规范化：0=Accepted / 1=Full（满员）/ 2=SchemaIncompatible（不兼容）/ 3=StateChanged（状态变化）。建立初步 LMK 后 Host↔Join 之间的后续 `PAIR_*` 包 SHALL 走 ESP-NOW 加密单播（硬件 AES-CCM）。

#### Scenario: beacon 周期广播
- **WHEN** Host 处于 `Hosting(CollectingPeers)`
- **THEN** 每 200ms(±20ms) 经 `send_broadcast(payload)` 广播一次 `PAIR_BEACON_HOST`，cur_members 字段反映当前已收集人数，joinable 字段在 members < MAX_GROUP_SIZE 时为 1 否则为 0

#### Scenario: 公钥交换与初步 LMK
- **WHEN** Host 收到 `PAIR_JOIN_REQ` 且 ver 校验通过并 accepted=1
- **THEN** Host 调用 `crypto.derive_lmk(host_priv, join_pub)` 得到 16B LMK，调用 `network.add_peer(join_mac, lmk)`；Join 收到 `PAIR_JOIN_ACK(accepted=1, reason=0)` 后对称执行 `derive_lmk(join_priv, host_pub) + add_peer(host_mac, lmk)`，两端的 LMK 值 SHALL 相等

#### Scenario: 公钥合法性校验
- **WHEN** Host 收到的 join_pub_key 全 0 或全 0xFF 或长度 ≠ 32
- **THEN** Host 单播 `PAIR_JOIN_ACK(accepted=0, reason=2=SchemaIncompatible)`，不追加成员

#### Scenario: 版本不兼容
- **WHEN** Host 收到 `PAIR_JOIN_REQ` 且包头 `ver != SCHEMA_VER`
- **THEN** Host 单播 `PAIR_JOIN_ACK(accepted=0, reason=2=SchemaIncompatible)`，不追加成员；非配对包 ver 不匹配时静默丢弃

### Requirement: 阶段2 大广播通讯录
阶段2 SHALL 在 Host 调用 `host_confirm()`（冻结点）后立即执行。Host SHALL 调用 `network.evaluate_channels([1,6,11,13])` 选定 target_channel，计算 `switch_offset: u16`（相对 ms），经定时器队列调度 5 次 `DIRECTORY_BROADCAST`（type=0x20，明文）广播（`now + 0, 80, 160, 240, 320 ms`，非阻塞，Task B 期间可处理 ACK/JOIN_REQ）。payload SHALL 含 member_count + mode + target_channel + switch_offset（2 字节大端）+ 成员条目数组（每项 38B = 6B mac + 32B pub_key）。冻结后 Host SHALL NOT 接受任何新的 `PAIR_JOIN_REQ`。

#### Scenario: 信道评估选择
- **WHEN** Host 调用 `host_confirm()`
- **THEN** 立即调用 `evaluate_channels([1,6,11,13])`，返回最低干扰信道作 target_channel；若所有信道 RSSI 均优于阈值则选第一个（信道 1）

#### Scenario: 通讯录定时器队列冗余广播
- **WHEN** Host 进入 `Hosting(Frozen)` 状态
- **THEN** 经定时器队列在 `now + 0, 80, 160, 240, 320 ms` 调度 5 次 `DIRECTORY_BROADCAST` 广播（非 `sleep` 阻塞），每次经 `send_broadcast(payload)` 发送，payload 完全相同（含相同的 switch_offset）；Task B 在广播间隙可处理 incoming ACK / PAIR_JOIN_REQ

#### Scenario: 冻结点不可逆
- **WHEN** Host 已进入 `Hosting(Frozen)` 状态
- **THEN** 后续收到的 `PAIR_JOIN_REQ` 一律拒绝（reason=3=StateChanged），members 列表不可再变更

### Requirement: 阶段3 切信道全对等重构
阶段3 SHALL 在到达调度时刻（Host 侧为下发时已知，Join 侧为 `now_join + switch_offset`）时由所有节点同步执行。每个节点 SHALL 按顺序执行：`network.clear_peers()` → `network.set_channel(target_channel)` → 对组内每个其他节点 `crypto.derive_lmk(my_priv, peer_pub)` + `network.add_peer(peer_mac, lmk)`。Join SHALL 在重构完成后向 Host 发送 `CHANNEL_SWITCH_ACK`（type=0x30，加密单播，经 `send_unicast(host_mac, payload)`），payload 含 sender_id + status(0=成功)。Host SHALL 等待全部成员 ACK 或 2s 超时后进入 `Grouped(Idle)`。

#### Scenario: LMK 重构顺序
- **WHEN** 节点到达调度时刻
- **THEN** 必须先 `clear_peers()` 再 `set_channel(target)` 再逐个 `add_peer`；反序执行会导致旧 LMK 残留，SHALL 被视为实现错误

#### Scenario: 全网状 LMK 对称性
- **WHEN** 节点 A 与节点 B 均完成阶段3 重构
- **THEN** A 的 peer[B] LMK 与 B 的 peer[A] LMK SHALL 相等（X25519 对称性）；4 节点组共 6 对 LMK，每节点注册 3 个 peer

#### Scenario: Join 发送切换确认
- **WHEN** Join 完成阶段3 LMK 重构
- **THEN** 向 Host 发送 `CHANNEL_SWITCH_ACK(status=0)`；若重构失败则 status 非 0

#### Scenario: Host ACK 超时容错
- **WHEN** Host 在 `Hosting(SwitchingChannel)` 状态等待 ACK，2s 内某成员未发送 `CHANNEL_SWITCH_ACK`
- **THEN** 该成员在本地标"待确认/离线"，但 Host 仍进入 `Grouped(Idle)`，组关系保留（不阻塞）

### Requirement: 组信息持久化
阶段3 完成后节点 SHALL 立即调用 `storage.save_group(group_info)`，group_info SHALL 含：本机 Curve25519 私钥（32B）、各对端 MAC↔公钥映射总表、当前工作信道、组模式。持久化 SHALL 在进入 `Grouped(Idle)` 之前完成。若 `save_group` 返回错误 SHALL 记录日志但不阻塞进入 `Grouped(Idle)`（降级：组关系已建立，仅冷启动恢复不可用）。

#### Scenario: 切信道后立即写 NVS
- **WHEN** 节点完成阶段3 `add_peer` 全部调用
- **THEN** 在状态转移到 `Grouped(Idle)` 之前调用 `save_group`，NVS namespace `group` 写入完整字段

#### Scenario: 写入失败不阻塞组关系
- **WHEN** `save_group` 返回 Err
- **THEN** 记录 `error!` 日志，状态仍转移到 `Grouped(Idle)`，组内通信正常；下次重启需重新组队（冷启动恢复降级）

### Requirement: 搜索主机列表数据契约
Join 处于 `Searching` 状态时 SHALL 通过共享状态暴露主机列表，每条 HostInfo SHALL 含：主机名称（默认 "Host-" + MAC 后 4 位）、MAC 后 4 位（轻量辅助标识）、信号 4 格（RSSI 平滑+滞回，不显示精确值）、当前人数 / 上限（cur_members / max_members）、当前模式（清晰/自由）、是否可加入（joinable）。5s 未刷新的条目 SHALL 从可加入列表移除。

#### Scenario: 列表字段完整
- **WHEN** UI 层读取主机列表
- **THEN** 每条 HostInfo 包含上述 6 个字段，缺一不可；信号值映射为 0~4 整数（4=最强）

#### Scenario: 主机结束移出列表
- **WHEN** 某主机 5s 内未再发送 `PAIR_BEACON_HOST`
- **THEN** 该条目标记 stale 并从可加入列表移除；若用户已选中该主机则提示"主机已结束组队"并提供重试入口

### Requirement: 失败场景与重试
`pairing.rs` SHALL 定义 `enum PairingFailure { SearchTimeout, HostEnded, GroupFull, SchemaIncompatible, SignalWeak, StateChanged }`。PAIR_JOIN_ACK reason 码规范化：0=Accepted / 1=Full（满员）/ 2=SchemaIncompatible（不兼容）/ 3=StateChanged（状态变化）。每个失败场景 SHALL 暴露明确失败原因码（非字符串）并提供重试入口（回到 `Joining(Searching)` 或 `Idle`）。

#### Scenario: 搜索超时
- **WHEN** Join 在 `Joining(Searching)` 状态 10s 未收到任何 beacon
- **THEN** 暴露 `PairingFailure::SearchTimeout`，UI 提示"搜索超时，未发现主机"，提供"继续等待"与"返回"入口

#### Scenario: 主机已结束
- **WHEN** Join 在 `Joining(Requesting)` 或 `Joining(WaitingConfirm)` 状态收到 `PAIR_JOIN_ACK(accepted=0, reason=3=StateChanged)` 或 DIRECTORY_BROADCAST 5s 超时未收到
- **THEN** 暴露 `PairingFailure::HostEnded`，状态回退 `Idle`，UI 提示"主机已结束组队"，提供重试入口

#### Scenario: 人数已满
- **WHEN** Join 收到 `PAIR_JOIN_ACK(accepted=0, reason=1=Full)`
- **THEN** 暴露 `PairingFailure::GroupFull`，UI 提示"该主机人数已满"，提供"搜索其他主机"入口

#### Scenario: schema 不兼容
- **WHEN** Host 检测到 Join 的公钥不合法（全 0 / 全 0xFF / 长度错）或包头 `ver != SCHEMA_VER`
- **THEN** Host 单播 `accepted=0, reason=2=SchemaIncompatible`；Join 暴露 `PairingFailure::SchemaIncompatible`，UI 提示"协议版本不兼容"，提供"返回"入口（无重试，需升级固件）

#### Scenario: 信号弱
- **WHEN** Join 在 `Joining(Searching)` 收到 beacon 但 RSSI 低于 -85dBm（映射为 1 格或 0 格）
- **THEN** 主机仍出现在列表但标注"信号弱"；用户选择时若 RSSI 持续低于阈值则暴露 `PairingFailure::SignalWeak`，UI 提示"信号弱，链路不稳定，建议靠近后重试"

### Requirement: 单 RF 半双工约束
组队状态机进入任何非 `Idle` 状态时 SHALL 设置 `radio_busy = true` 标志，禁止 OTA / BLE / Wi-Fi 扫描等并发无线活动。进入 `Grouped(Idle)` 或回退 `Idle` 时 SHALL 释放该标志。

#### Scenario: 组队期间禁止 OTA
- **WHEN** 设备处于 `Hosting(CollectingPeers)` 状态且 UI 层尝试触发 OTA 检查
- **THEN** OTA 入口被禁用（`radio_busy` 为 true），提示"组队中，无法执行 OTA"

#### Scenario: 组队完成释放无线
- **WHEN** 状态转移到 `Grouped(Idle)`
- **THEN** `radio_busy` 置 false，OTA/BLE 等无线功能恢复可用
