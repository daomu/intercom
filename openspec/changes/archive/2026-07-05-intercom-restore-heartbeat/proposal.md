## Why

17 个变更提案的第 12 个。前期变更 03/06/09 已分别落地 StorageService（`load_group`/`clear_group`）、NetworkService（`init(channel)`/`add_peer(mac, lmk)`）、三阶段组队（Grouped 状态 + peer 列表）。但冷启动时如何"瞬时、无空中交互"地恢复到 Grouped(Idle)、以及组网后成员在线状态如何低开销持续维护，尚未实现。本变更新增冷启动 NVS 恢复流程与去中心化心跳任务，使设备上电即可就绪待机、并能在运行期持续反映对端在线状态与 4 格信号。

## What Changes

- 新增 `src/intercom/restore.rs`：实现 `restore_from_nvs()` 冷启动恢复——`storage.load_group()` → 若 `Some(g)`：`network.init(g.channel)`（直接到工作信道，不经 ch1）→ 对每个 peer `crypto.derive_lmk(g.my_priv_key, peer.pub_key)` → `network.add_peer(peer.mac, lmk)` → 进入 `Grouped(Idle)`；全程零 RF 交互
- 组信息损坏 / schema 不兼容 / `load_group` 返回 `None` → `storage.clear_group()` → 状态保持 `Idle`（未组网），不阻塞启动
- 新增 `src/intercom/heartbeat.rs`：心跳任务，按状态相关周期发送 HEARTBEAT 包，并对接收到的对端心跳/消息做在线状态与 RSSI 更新
- 心跳周期（技术设计 §13.1）：已组网待机 5s、组队中 1s、稳定语音收发 10s、熄屏低功耗待机 10s
- 离线判定（§13.2）：15s 未收到某对端心跳/消息 → 标离线（仅标记，不删除成员表条目）
- 恢复在线：重新收到该对端任意消息/心跳 → 立即标在线
- RSSI 4 格平滑 + 滞回：对端心跳/数据包携带的 RSSI 经指数平滑 + 滞回阈值映射到 0–4 格，避免边界处频繁跳变
- 去中心化 Peers 拓扑：每台设备独立维护成员状态表，收到对端心跳即本地更新该节点 `online=true` + `signal=4bar`
- 在 `IntercomService` 实现中串联：`restore_from_nvs()` 成功后启动心跳任务；`leave_group()` / 损坏清理时停止心跳任务
- 心跳任务运行在 Task B（网络业务，优先级 12 / 栈 6KB），集成在 NetworkService recv 分发循环中，与组队/语音收发共用 task，不新增 task

## Capabilities

### New Capabilities
- `intercom-restore`: 冷启动从 NVS 恢复组信息 + LMK 加密表 + Grouped(Idle) 状态，全程无 RF 交互
- `intercom-heartbeat`: 状态相关周期心跳发送、对端在线/离线判定、RSSI 4 格平滑+滞回、去中心化 Peers 状态维护

### Modified Capabilities
<!-- 本变更不修改既有 capability 的 spec 级行为；restore/heartbeat 均为新增 -->

## Impact

- **代码**：新增 `src/intercom/restore.rs`、`src/intercom/heartbeat.rs`；在 `src/intercom/mod.rs` 注册两个子模块；`IntercomService` 实现中调用 `restore_from_nvs()` 与心跳任务启停
- **依赖**：deps = `03, 06, 08, 09`。复用 change 03 的 `StorageService::load_group`/`clear_group` 与 `CryptoService::derive_lmk`；change 06 的 `NetworkService::init`/`add_peer`；change 08 的 HEARTBEAT 包编解码（type=0x02，sender_id/state/mode 字段）；change 09 的 `Grouped` 状态与 peer 表
- **任务调度**：心跳任务挂载到 Task B（网络业务 task），不新增 task
- **后续变更**：change 13（App UI）消费 `PeerOnline`/`PeerOffline`/`PeerOnline(id, rssi_4)` 事件渲染成员卡片；change 15（电源管理）依据熄屏状态切换心跳周期到 10s
- **无 RF 行为**：restore 路径零无线电交互；heartbeat 路径发送 HEARTBEAT 包属于正常业务 RF
