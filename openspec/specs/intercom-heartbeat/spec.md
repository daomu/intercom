## ADDED Requirements

### Requirement: 状态相关心跳周期
心跳任务 SHALL 按当前 `IntercomState` 与熄屏标记决定发送周期：`Grouped(Idle)` 或 `Grouped(ChannelBusy)` → 5s；`Hosting(_)` 或 `Joining(_)` → 1s；`Grouped(Talking)` 或 `Grouped(Listening)` → 10s；熄屏待机标记被设置时 → 10s。状态切换时 SHALL 立即重排周期（不等当前周期到期）。

#### Scenario: 待机 5s 心跳
- **WHEN** 设备处于 `Grouped(Idle)`
- **THEN** 每 5s 发送一个 HEARTBEAT 包

#### Scenario: 组队中 1s 心跳
- **WHEN** 设备处于 `Hosting(Discovering/CollectingPeers/Frozen/SwitchingChannel)` 或 `Joining(Searching/Requesting/WaitingConfirm/SwitchingChannel)`
- **THEN** 心跳周期切换为 1s

#### Scenario: 稳定语音 10s 心跳
- **WHEN** 设备进入 `Grouped(Talking)` 或 `Grouped(Listening)`
- **THEN** 心跳周期切换为 10s（语音活动本身维持在线判定，无需高频心跳）

#### Scenario: 熄屏待机 10s 心跳
- **WHEN** 熄屏待机标记被设置（由 change 15 电源管理触发）
- **THEN** 心跳周期切换为 10s，无论 VoiceState 为何

#### Scenario: 状态切换立即重排周期
- **WHEN** 设备从 `Grouped(Idle)` 切换到 `Grouped(Talking)`
- **THEN** 心跳任务立即按 10s 周期重排，不等当前 5s 周期到期

### Requirement: 离线判定 15s 仅标记不删除
对组内每个 peer，若连续 15s 未收到该 peer 的任何 HEARTBEAT / VOICE / TALK_STATE / CTRL 包，SHALL 将该 peer 标记为离线；SHALL NOT 从成员表删除该 peer 条目。

#### Scenario: 心跳超时标离线
- **WHEN** 某 peer 超过 15s 未发送任何包到本机
- **THEN** 该 peer 被标记离线，触发 `PeerOffline(id)` 事件，成员表仍保留该 peer

#### Scenario: 离线成员不自动删除
- **WHEN** 某 peer 已被标记离线
- **THEN** 成员表条目保留，peer 不被自动移除；正式移除需整组解散重建

### Requirement: 恢复在线判定
已标记离线的 peer，当本机重新收到该 peer 的任意消息（HEARTBEAT / VOICE / TALK_STATE / CTRL）时，SHALL 立即将其标记为在线，触发 `PeerOnline(id, rssi_4)` 事件，重置其 `last_seen` 时间戳。

#### Scenario: 离线 peer 重新发消息
- **WHEN** 本机收到一个已离线 peer 的任意包
- **THEN** 该 peer 立即被标记在线，`PeerOnline(id, rssi_4)` 事件发出

#### Scenario: 语音活动维持在线
- **WHEN** 稳定语音收发期间（心跳周期 10s），本机持续收到某 peer 的 VOICE 包
- **THEN** 该 peer 的 `last_seen` 持续更新，不因心跳周期拉长而误判离线

### Requirement: RSSI 4 格平滑与滞回
本机 SHALL 对每个组内 peer 维护一个 RSSI 平滑值（指数加权移动平均 EWMA，α=0.3），并按滞回阈值映射到 0–4 格。映射阈值：`≥-55dBm→4, ≥-65→3, ≥-75→2, ≥-85→1, <-85→0`。滞回规则：当前格为 N 时，仅当平滑值跨过相邻档阈值 ±3dB 才切换显示格数。SHALL NOT 暴露精确 RSSI 数值给 UI。

#### Scenario: 平滑后稳定显示
- **WHEN** 某 peer RSSI 在 -64 ~ -66dBm 间抖动（第 3/4 档边界）
- **THEN** 经 EWMA 平滑 + 滞回后显示格数稳定，不频繁跳变

#### Scenario: 跨档切换
- **WHEN** 平滑 RSSI 从 -60 持续下降到 -72
- **THEN** 显示格数从 4 切换到 2（经过 -65+3=-62 阈值降到 3，再经过 -75+3=-72 阈值降到 2）

#### Scenario: 不暴露精确值
- **WHEN** UI 查询某 peer 信号
- **THEN** 仅返回 0–4 整数格，不返回 dBm 浮点值

### Requirement: 去中心化 Peers 拓扑
每台设备 SHALL 独立维护组内其他成员的在线状态与 4 格信号；SHALL NOT 依赖中心节点转发成员状态。收到对端 HEARTBEAT 包即本地更新该节点 `online=true` 与 `signal`。

#### Scenario: 收到对端心跳本地更新
- **WHEN** 本机收到组内 peer A 的 HEARTBEAT 包
- **THEN** 本机本地将 peer A 标记在线并更新其 4 格信号，无需任何中心节点参与

#### Scenario: 无中心协调者
- **WHEN** 组内 4 台设备运行中
- **THEN** 每台设备各自独立维护其余 3 台的在线/信号状态，无 host 角色负责状态汇总

### Requirement: 心跳包格式
心跳任务发送的 HEARTBEAT 包 SHALL 遵循 change 08 定义的 type=0x02 格式：8 字节 header（含 seq）、1 字节 sender_id、1 字节 state（本机 VoiceState）、1 字节 mode（清晰/自由）。`state` 字段取值：0=Idle、1=Talking、2=Listening、3=ChannelBusy。注：技术设计 §4.3 仅列出 Idle/Talking/Listening 三态，但 `VoiceState` 的第四个变体 `ChannelBusy`（D44）应映射为 3。

#### Scenario: 包字段正确
- **WHEN** 心跳任务发送 HEARTBEAT 包
- **THEN** sender_id 为本机在组内的 id，state 反映当前 VoiceState（0/1/2/3 之一），mode 反映当前 IntercomMode

### Requirement: 心跳任务生命周期
心跳任务 SHALL 在进入 `Grouped` 状态时启动，在离开 `Grouped`（`leave_group` / 损坏清理 / 恢复失败回 Idle）时停止。任务 SHALL 运行在 Task B（网络业务 task，优先级 12 / 栈 6KB），不新增 task。

#### Scenario: 进入 Grouped 启动
- **WHEN** 设备从 `Idle` 进入 `Grouped(Idle)`（无论经 restore 还是组队完成）
- **THEN** 心跳任务启动

#### Scenario: 离开 Grouped 停止
- **WHEN** 设备从 `Grouped` 切回 `Idle`
- **THEN** 心跳任务停止，不再发送 HEARTBEAT 包
