## Context

仓库当前已完成 change 01（工程骨架）、change 02（BSP 驱动）、change 03（Storage+Crypto Service）、change 04（Display/Input/Power Service）、change 06（ESP-NOW NetworkService）与 change 08（包格式定义）。设备已能：
- `NetworkService::init(channel)` / `add_peer(mac, lmk)` / `clear_peers()` / `set_channel(ch)` / `evaluate_channels(candidates)` / `send_broadcast(payload)` / `send_unicast(dst, payload)` / 注册 recv 回调
- `CryptoService::gen_keypair() -> {priv, pub}` / `derive_lmk(my_priv, peer_pub) -> [u8;16]`（X25519 + HKDF-SHA256，salt="ESP32C6-INTERCOM", info="LMK-v1"）
- `StorageService::save_group(g)` / `load_group() -> Option<GroupInfo>`（NVS namespace `group`）
- packet encode/parse：`encode_pair_beacon_host(...)` / `parse_pair_beacon_host(...)` 等 5 个配对包函数 + `PairingPacket` 枚举（type 0x10/0x11/0x12/0x20/0x30）

但尚无任何组队业务逻辑：`src/intercom/pairing.rs` 不存在，`src/intercom/mod.rs` 仍为空占位。设备上电后停在未组网页，无法创建主机或搜索加入。

目标硬件：2 × Waveshare ESP32-C6-Touch-LCD-1.54（ESP32-C6 RISC-V 160MHz / 512KB SRAM / 16MB Flash / 单 RF）。单 RF 半双工约束：ESP-NOW 与 Wi-Fi 共享一个 radio，组队期间禁止 OTA/BLE 等并发无线活动。三 task 并发模型已固化（Task A 音频 19/8KB、Task B 网络业务 12/6KB、Task C UI 5/12KB）——本期组队逻辑运行在 Task B，UI 交互在 Task C。

## Goals / Non-Goals

**Goals:**
- 实现 Host 状态机：`Discovering → CollectingPeers → Frozen → SwitchingChannel → Grouped(Idle)`，覆盖技术设计 §19.1 全部伪代码路径
- 实现 Join 状态机：`Searching → Requesting → WaitingConfirm → SwitchingChannel → Grouped(Idle)`，覆盖技术设计 §19.2 全部伪代码路径
- 阶段1：Host 200ms 周期广播 PAIR_BEACON_HOST；Join 单播 PAIR_JOIN_REQ；Host 单播 PAIR_JOIN_ACK；建立 Host↔Join 初步 LMK
- 阶段2：Host 确认后冻结成员、evaluate_channels([1,6,11,13]) 选 target、计算 `switch_offset: u16`（相对 ms）、经定时器队列调度 DIRECTORY_BROADCAST ×5（`now + 0, 80, 160, 240, 320 ms`，非阻塞）
- 阶段3：到调度时刻（`now_join + switch_offset`）全员 clear_peers + set_channel(target) + 全网状 LMK 重构 + Join 发 CHANNEL_SWITCH_ACK + Host 收齐/超时 → Grouped(Idle)
- 切信道成功后立即 save_group（私钥+公钥总表+信道+模式），遵循 PRD §18.6
- Host 组队超时 60~120s 未确认 → 自动取消 → `IntercomState::Idle`，提示"组队超时，已结束"
- 搜索主机列表数据契约：名称 + MAC 后 4 位 + 信号 4 格 + 人数/上限 + 模式 + joinable
- 失败场景覆盖（PRD §11.7）：搜索超时 / 主机已结束 / 已满 / schema 不兼容 / 信号弱，均带明确原因 + 重试入口
- 2 台设备验收：1 Host + 1 Join 完成三阶段闭环进入 Grouped(Idle)

**Non-Goals:**
- PTT 语音收发 → change 10
- 心跳与在线状态判定 → change 12
- 真实 UI 页面渲染（搜索列表/创建主机/Join 确认/失败提示页） → change 13；本期仅暴露数据契约与触发接口供 UI 层消费
- 冷启动恢复（restore_from_nvs）的运行时实现 → change 12；本期仅在阶段3 调用 save_group 写入 NVS，恢复路径由 change 12 实现
- 4 节点全 mesh 验收 → 集成变更；本期 2 节点闭环即可，但 LMK 表设计须支持 4 节点（6 对）
- 在线增删员 / 在线切信道 / 在线改模式 → 技术设计 §12.3 明确不支持
- 抗 MitM / 验证码 SAS / 单设备密码学吊销 → PRD §18.2 明确不承诺
- 变声器、低功耗管理、诊断 → 后续变更

## Decisions

### D1：状态机实现 = enum + 不可变状态转移（无 async）
组队状态复用 change 08 的 `IntercomState` 枚举（不再定义独立 `PairingState`）：`IntercomState::Idle`（未组网）/ `Hosting(HostPhase)` / `Joining(JoinPhase)` / `Grouped(VoiceState)`。`HostPhase` 变体：`Discovering` / `CollectingPeers { members: Vec<PeerEntry> }` / `Frozen { members: Vec<PeerEntry>, target: u8, offset: u16 }` / `SwitchingChannel { target: u8, deadline: Instant }`。`JoinPhase` 变体：`Searching { hosts }` / `Requesting { host }` / `WaitingConfirm { host }` / `SwitchingChannel { target, deadline }`。`Grouped(Idle)` 表示已组网待机。状态转移在 Task B 的 recv 回调 + 定时器中同步驱动，不引入 async runtime（esp-idf-svc std 无原生 async，且组队逻辑是短时序强状态机，async 增加复杂度无收益）。备选：async-std / embassy —— esp-idf-svc std 模式下引入 embassy 与现有 blocking API 冲突，排除。

### D2：阶段1 beacon 周期 = 200ms 固定（不做自适应）
PAIR_BEACON_HOST 每 200ms 广播一次，覆盖 Join 搜索窗口（Join 进入 Searching 后最多 3s 内应收到至少一个 beacon）。不做按 Join 数量自适应——组队期短（<120s）、节点少（≤4）、固定周期可预测、不与后续语音包抢占空口。备选：自适应 100~500ms —— 复杂度上升、收益不明显，排除。

### D3：阶段2 DIRECTORY_BROADCAST ×5 / 80ms 间隔 = 定时器队列调度（非阻塞）
技术设计 §5.5 注释「200~500ms 区间连续广播 3~5 次」。固定取 5 次广播，经定时器队列调度在 `now + 0, 80, 160, 240, 320 ms` 触发（非 `sleep(80ms) × 5` 阻塞），Task B 在广播间隙仍可处理 incoming ACK / PAIR_JOIN_REQ。冗余窗口 320ms 提高单包丢失场景下的到达率。备选：3 次 ×100ms —— 冗余不足，单包丢失败率高，排除。

### D4：switch_offset 用相对 ms（非绝对时间戳）
DIRECTORY_BROADCAST 下发 `switch_offset: u16`（相对 ms，2 字节），Join 在收到首个 DIRECTORY_BROADCAST 时按 `now_join + switch_offset` 调度切信道时刻；后续冗余广播忽略（offset 已过期则不刷新）。相对 ms 消除跨节点 mono 时钟域差异，且比绝对 u32 时间戳省 2 字节。备选：NTP 时钟同步 —— 单 RF 半双工无 Wi-Fi 基础设施，排除；广播倒计数包 —— 增加空口占用且 Join 错过中间包则失步，排除。

### D5：阶段3 LMK 重构顺序 = clear_peers → set_channel → 逐个 add_peer
严格遵循技术设计 §6.3 / §19.1 顺序：先 `clear_peers()` 清除阶段1 仅 Host↔Join 的初步 LMK，再 `set_channel(target)`（ESP-NOW peer 表与 channel 绑定，切 channel 后旧 peer 失效须重建），最后对组内每个其他节点 `derive_lmk + add_peer`。备选：先 add 新 peer 再 clear 旧 peer —— ESP-NOW peer 表按 mac 去重，clear 后 add 才能保证 LMK 已更新，反序会复用旧 LMK，排除。

### D6：CHANNEL_SWITCH_ACK 超时 = 2s，超时不阻塞进入 Grouped(Idle)
Join 切信道后向 Host 发 CHANNEL_SWITCH_ACK(status=0)。Host 在 SwitchingChannel 状态等 ACK，收齐或 2s 超时后进入 Grouped(Idle)——超时未 ACK 的成员在本地标"待确认"，但不阻塞组关系建立（技术设计 §5.6 注释「超时未收到 ACK 则该成员标离线，组关系仍保留」）。备选：无限等待 —— 单节点故障导致整组卡死，排除；严格 ACK 才进入 Grouped —— 容错不足，排除。

### D7：Host 组队超时 = 90s（60~120s 区间取中值）
进入 CollectingPeers 后启动 90s 定时器，期间 Host 未调用 `host_confirm()` 则自动 cancel → 回 `IntercomState::Idle`，UI 提示"组队超时，已结束"。取 90s 是 60~120s 区间的中值，平衡用户操作裕度与设备占用空口时间。备选：用户可配置 —— 增加设置项复杂度无收益，排除。

### D8：用户取消仅限 Discovering / CollectingPeers（Frozen 不可取消）
用户主动取消仅在 `Hosting(Discovering)` / `Hosting(CollectingPeers)` 状态允许——一旦进入 `Frozen`（已下发 switch_offset + DIRECTORY_BROADCAST），不可取消，须走完阶段3 或等 ACK 超时 / 组队超时兜底。Frozen 后中途取消会导致 Join 已切信道但 Host 已废弃，造成孤岛。备选：Frozen 也可取消 —— 已下发切换指令后取消会造成节点失步，排除。

### D9：搜索主机列表 = 缓存 + 去重 + 5s 过期
Join 在 Searching 状态收到 PAIR_BEACON_HOST 后，按 host_mac 去重更新本地 `hosts: HashMap<Mac, HostInfo>`。HostInfo 包含 name(默认 "Host-"+MAC后4位) + mac_suffix4 + rssi_4bar(平滑+滞回，技术设计 §13.2) + cur_members + max_members + mode + joinable + last_seen_ms。5s 未再收到 beacon 的条目标记 stale 并从可加入列表移除（视为"主机已结束"）。备选：仅保留最新一条 —— 用户无法选择多主机，排除；永久缓存 —— 已结束的主机仍出现在列表，排除。

### D10：失败原因码 = enum 显式枚举（非字符串）
定义 `enum PairingFailure { SearchTimeout, HostEnded, GroupFull, SchemaIncompatible, SignalWeak, StateChanged }`，每个失败码映射到固定 UI 文案。PAIR_JOIN_ACK reason 码规范化：0=Accepted / 1=Full（满员）/ 2=SchemaIncompatible（不兼容）/ 3=StateChanged（状态变化）。备选：String 错误信息 —— 嵌入式无堆栈字符串处理开销且 i18n 困难，排除。

### D11：组队逻辑运行在 Task B（网络业务 task）
组队期间 Task A（音频）空闲、Task C（UI）仅刷新列表/状态。PAIR_* 包收发、状态转移、定时器均在 Task B 的 NetworkService recv 回调 + 一个 100ms tick 定时器中驱动。UI 层通过 `PairingHandle` 共享状态（`Arc<Mutex<IntercomState>>`）读取。备选：组队期独占 task —— 三 task 模型已固化，新增 task 违反 change 01 D10 决策，排除。

### D12：初步 LMK 与最终 LMK = 同算法不同时机
阶段1 Host↔Join 的"初步 LMK"与阶段3 全网状的"最终 LMK"用相同 `derive_lmk(my_priv, peer_pub)` 算法——区别仅在时机与 peer 范围：阶段1 仅 Host↔该 Join 一对，阶段3 为组内所有其他节点。阶段3 clear_peers 后重新 add_peer 时，Host↔Join 的 LMK 值与阶段1 相同（同一密钥对），但 peer 表已清空重建，无残留。备选：阶段1 用临时弱密钥、阶段3 换强密钥 —— 增加密钥派生分支、无安全收益（阶段1 包均为明文控制包），排除。

## Risks / Trade-offs

- **[单 RF 半双工：组队期间 OTA/BLE 不可并发]** → 组队状态机进入时设置 `radio_busy = true` 标志，UI 层禁用 OTA 入口；组队完成或取消后释放
- **[DIRECTORY_BROADCAST 单包丢失导致 Join 错过切信道]** → D3 定时器队列调度 5 次冗余广播缓解；若 5 包全丢则 Join 停在 WaitingConfirm 超时 → 提示"主机已结束"，重试
- **[switch_offset 相对时序偏差]** → Join 收到首个 DIRECTORY_BROADCAST 后立即按 `now_join + switch_offset` 调度，offset 远大于空口往返时延（<50ms），偏差可忽略；极端情况下 Join 延迟切信道 → Host 在 D6 的 2s ACK 超时内仍可收到迟到的 ACK
- **[4 节点全 mesh 未在 2 台设备上验证]** → LMK 表设计支持 6 对，但本期仅 1 Host + 1 Join 验证 1 对；4 节点闭环留待集成变更，风险为 LMK 表索引错误——通过单元测试 `derive_lmk` 对称性（A→B == B→A）覆盖
- **[Host 在阶段2 评估信道时 Join 突然离线]** → DIRECTORY_BROADCAST 已含该 Join 的条目，Join 不切信道则 Host 在 D6 超时后标该 Join 离线，组关系仍建立（其余成员正常）；后续 change 12 心跳会清理
- **[组队超时定时器与状态转移竞态]** → 状态转移与定时器均在 Task B 单线程内，无锁竞态；UI 层仅读不写
- **[curve25519-dalek 在 RISC-V 的 derive_lmk 耗时]** → 单次 X25519+HKDF 约 10~30ms，4 节点 3 对约 90ms，在 500ms switch 窗口内可完成；若超时则 D6 的 2s ACK 超时兜底
- **[NVS 写入失败导致冷启动无法恢复]** → save_group 返回 Err 时记录日志但不阻塞进入 Grouped(Idle)（组关系已建立，仅持久化失败）；下次重启无法恢复需重新组队——可接受的降级

## Migration Plan

无既有组队运行时需要迁移。部署步骤：
1. 拉取本变更 commit（依赖 change 03/06/08 已合并；deps = `03, 06, 08`）
2. `cargo build` 验证编译
3. 烧录 2 台设备
4. 设备 A：进入对讲 → 创建主机 → 选模式（清晰）→ 进入 CollectingPeers
5. 设备 B：进入对讲 → 搜索主机 → 列表出现 A → 选择 → 发 PAIR_JOIN_REQ → 收到 PAIR_JOIN_ACK → WaitingConfirm
6. 设备 A：点击确认 → Frozen → DIRECTORY_BROADCAST（定时器队列 ×5）→ 切信道 → Grouped(Idle)
7. 设备 B：收到首个 DIRECTORY_BROADCAST → 按 `now_join + switch_offset` 调度切信道 → CHANNEL_SWITCH_ACK → Grouped(Idle)
8. 验证 NVS 已写入 group 信息（重启后由 change 12 恢复逻辑读取，本期仅验证写入不报错）

回滚：`git revert <commit>`，pairing.rs 移除后 `src/intercom/mod.rs` 回到空占位，设备回未组网页。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
