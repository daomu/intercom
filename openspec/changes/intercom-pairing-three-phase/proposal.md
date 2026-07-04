## Why

17 个变更提案的第 9 个（change 09/17）。前序 change 06（ESP-NOW 网络服务）与 change 08（包格式定义）就绪后，设备已能收发明文包并枚举 packet type，但尚无任何组队业务逻辑：无法创建主机、搜索加入、交换公钥、协商信道、重构全网状 LMK、进入已组网待机。本期实现技术设计 §5 / §19.1 / §19.2 与 PRD §11 的三阶段 ECDH 全网状组队状态机，使两台 Waveshare ESP32-C6-Touch-LCD-1.54 能完成「创建主机 → Join 搜索 → 公钥收集 → Host 确认 → 通讯录大广播 → 同步切信道 → 全对等 LMK 重构 → 已组网待机」的完整闭环，为后续 change 10（PTT 语音）/ 12（恢复心跳）奠定组关系基础。

## What Changes

- 新增 `src/intercom/pairing.rs`：实现 Host 状态机 `Discovering → CollectingPeers → Frozen → SwitchingChannel → Grouped(Idle)` 与 Join 状态机 `Searching → Requesting → WaitingConfirm → SwitchingChannel → Grouped(Idle)`
- 阶段1（1信道星型公钥收集）：Host 在 DISCOVERY_CHANNEL(=1) 周期 200ms 广播 `PAIR_BEACON_HOST`（type=0x10）；Join 收到 beacon 后单播 `PAIR_JOIN_REQ`（type=0x11）；Host 在发送 `PAIR_JOIN_ACK` 前先校验包头 `ver` 字段：`ver != SCHEMA_VER` 时发送 `PAIR_JOIN_ACK(accepted=0, reason=2=SchemaIncompatible)`，不追加成员；非配对包 ver 不匹配时静默丢弃；Host 单播 `PAIR_JOIN_ACK`（type=0x12，含 accepted/reason，reason 码：0=Accepted / 1=Full / 2=SchemaIncompatible / 3=StateChanged）；Host↔Join 用 ECDH 派生"初步 LMK"并 `add_peer` 写入硬件（仅 Host↔Join 点对点，Join 之间尚未互通）
- 阶段2（1信道大广播通讯录）：Host 点击确认 → 冻结成员列表（冻结点 = host_confirm 调用）→ `evaluate_channels([1,6,11,13])` 选定 target_channel → 计算 `switch_offset: u16`（相对 ms，下发后 Join 在收到首个 DIRECTORY_BROADCAST 时按 `now_join + switch_offset` 调度切信道；后续冗余广播忽略）→ 经定时器队列调度 5 次 `DIRECTORY_BROADCAST`（type=0x20）广播（`now + 0, 80, 160, 240, 320 ms`，非阻塞，Task B 期间可处理 ACK/JOIN_REQ），payload 含 member_count + mode + target_channel + switch_offset（2 字节大端）+ 成员条目数组（mac+pub_key, 38B/项）
- 阶段3（切信道全对等重构）：到调度时刻（`now_join + switch_offset`）全员 `clear_peers` → `set_channel(target)` → 对组内每个其他节点 `derive_lmk(my_priv, peer_pub)` + `add_peer(mac, lmk)` 重构全网状（4 节点 = 6 对 LMK）→ Join 向 Host 发 `CHANNEL_SWITCH_ACK`（type=0x30，加密单播）→ Host 收齐 ACK 或超时 → 进入 `Grouped(Idle)`
- 持久化：切信道成功后立即 `storage.save_group(group_info)`（本机私钥 + 对端公钥/MAC 总表 + 当前信道 + 模式），遵循 PRD §18.6「迁移工作信道前强制写 NVS」
- Host 组队超时：进入 CollectingPeers 后 60~120s 未确认 → 自动取消 → 回 Idle，UI 提示"组队超时，已结束"
- 搜索主机列表 UI 数据契约：暴露主机名称 + MAC 后 4 位 + 信号 4 格 + 当前人数/上限 + 模式 + joinable 标志（遵循 PRD §11.5）；创建主机 + 模式选择 UI 入口；Join 确认页
- 失败场景覆盖（PRD §11.7）：搜索超时 / 主机已结束 / 人数已满 / schema 不兼容 / 信号弱·链路不稳定——均需明确失败原因 + 重试入口
- 依赖 change 08 的 packet encode/parse（`encode_pair_beacon_host`/`parse_pair_beacon_host` / `encode_pair_join_req`/`parse_pair_join_req` / `encode_pair_join_ack`/`parse_pair_join_ack` / `encode_directory_broadcast`/`parse_directory_broadcast` / `encode_channel_switch_ack`/`parse_channel_switch_ack`）与 packet type 枚举
- 依赖 change 06 的 `NetworkService`（init / set_channel / add_peer / clear_peers / evaluate_channels / send_broadcast / send_unicast / recv 回调）与 change 03 的 `CryptoService`（gen_keypair / derive_lmk）+ `StorageService`（save_group / load_group）

## Capabilities

### New Capabilities
- `intercom-pairing`: 三阶段 ECDH 全网状组队状态机（Host/Join 双侧）、信道协商、成员冻结、切信道全对等 LMK 重构、组信息持久化、组队超时与失败场景处理

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 intercom-pairing -->

## Impact

- **代码**：新增 `src/intercom/pairing.rs`（Host/Join 状态机 + 阶段1~3 逻辑）；在 `src/intercom/mod.rs` 注册 `pub mod pairing;`
- **依赖**：deps = `03, 06, 08`。调用 change 03 `CryptoService`（gen_keypair/derive_lmk）与 `StorageService`（save_group/load_group，GroupInfo 持久化）、change 06 `NetworkService` API（init/set_channel/add_peer/clear_peers/evaluate_channels/send_broadcast/send_unicast/recv）、change 08 packet encode/parse 函数（`encode_pair_beacon_host`/`parse_pair_beacon_host` 等）与 `PairingPacket` 枚举
- **后续变更**：change 10（PTT 语音）依赖 `Grouped(Idle)` 状态与全网状 LMK；change 12（恢复心跳）依赖 `save_group` 持久化字段；change 13（App UI）消费搜索主机列表数据契约与失败提示
- **硬件**：2 × Waveshare ESP32-C6-Touch-LCD-1.54 验收（1 Host + 1 Join 最小闭环；4 节点全 mesh 在后续集成变更验证）
- **无 BSP 驱动变更**：不修改 ST7789/CST816/ES7210/ES8311 驱动
- **无包格式变更**：PAIR_* 包结构由 change 08 定义，本期仅消费
