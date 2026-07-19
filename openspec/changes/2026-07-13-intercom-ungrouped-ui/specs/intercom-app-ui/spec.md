## MODIFIED Requirements

### Requirement: 未组网主入口页
`IntercomPage` SHALL 新增 `UngroupedHome` 变体。未组网时 IntercomApp 默认 page = UngroupedHome。该页 SHALL 显示：(a) 标题 "No Group Yet"；(b) 大按钮 "Create Group (Become Host)"；(c) 大按钮 "Search Groups (Join Existing)"；(d) 简短说明 "Host 在线时其他可加入"。SHALL NOT 仅显示 placeholder 文本（如原 "No Group / Use Settings to create or join"）。

#### Scenario: 未组网默认入口
- **WHEN** 设备启动且无 group state 恢复，IntercomApp 初始化
- **THEN** page = UngroupedHome，渲染显示两个大按钮 + 说明，用户可点 create 或 search

#### Scenario: 点 Create Group 进入创建流程
- **WHEN** 用户在 UngroupedHome 点 "Create Group" 按钮
- **THEN** hit_test 命中 CreateHostButton，dispatch 返回 pairing_action=StartHost，page 切到 CreatingHost，intercom_svc.set_state(Hosting(Advertising)) 启动广播

#### Scenario: 点 Search Groups 进入搜索流程
- **WHEN** 用户在 UngroupedHome 点 "Search Groups" 按钮
- **THEN** hit_test 命中 SearchHostsButton，dispatch 返回 pairing_action=SearchHosts，page 切到 SearchingHosts，discovered_hosts 清空，intercom_svc.set_state(Joining(Scanning)) 启动扫描

### Requirement: 创建群组流程页
`IntercomPage::CreatingHost` SHALL 显示：(a) 标题 "Creating Group..."；(b) spinner 动画（基于 tick_count 闪烁圆点）；(c) "Broadcasting..." 文字；(d) 已加入 peer 计数（实时更新）；(e) Back 按钮。Back 触发 `pairing_action=Cancel` + 切回 UngroupedHome + `intercom_svc.set_state(Idle)` 停止广播。

#### Scenario: peer 加入计数更新
- **WHEN** host 广播期间有 peer 加入，收到 IntercomEvent::PeerJoined
- **THEN** CreatingHost 页 `creating_peer_count += 1`，下一 tick 渲染显示更新后的计数

#### Scenario: Back 取消创建
- **WHEN** 用户在 CreatingHost 点 Back
- **THEN** hit_test 命中 CancelButton，dispatch 返回 pairing_action=Cancel，page 切回 UngroupedHome，intercom_svc 停止广播

### Requirement: 搜索群组流程页
`IntercomPage::SearchingHosts` SHALL 显示：(a) 标题 "Searching Groups..."；(b) discovered_hosts 列表（按 rssi 降序排序，最多 8 项，每项显示 host_id 简称 + 信号格数）；(c) Refresh 按钮（重新扫描）；(d) Join 按钮（选中后高亮）；(e) Back 按钮。点击列表项选中 `selected_host = Some(idx)`，Join 按钮在 selected 时触发 `pairing_action=Join(host_id)` + 切 page=JoiningHost。

#### Scenario: 发现 host 列表更新
- **WHEN** 收到 IntercomEvent::HostDiscovered(host_id, rssi)
- **THEN** discovered_hosts.push(HostInfo{...})，按 rssi 降序排序，下一 tick 渲染列表刷新

#### Scenario: 选中 host 后 Join
- **WHEN** 用户点击列表第 2 项（selected_host=Some(1)），然后点 Join 按钮
- **THEN** hit_test 命中 JoinButton + selected_host.is_some()，dispatch 返回 pairing_action=Join(host_id)，page 切到 JoiningHost

#### Scenario: 列表最多 8 项
- **WHEN** 收到第 9 个 HostDiscovered 事件
- **THEN** 列表已满，新项被丢弃 + log::debug，SHALL NOT 溢出

### Requirement: 加入群组确认页
`IntercomPage::JoiningHost(HostId)` SHALL 显示：(a) 标题 "Joining {host_id}..."；(b) "Waiting for approval" 文字；(c) spinner；(d) Cancel 按钮。`IntercomEvent::JoinAccepted` → 切 page=Main（spec：加入成功立即进入主对讲页）。`JoinRejected` → 切回 UngroupedHome + 显示 join_error toast。

#### Scenario: 加入成功进 Main
- **WHEN** 收到 IntercomEvent::JoinAccepted
- **THEN** page 切到 Main（grouped 主对讲页），peers 初始化为 host 提供的列表

#### Scenario: 加入被拒
- **WHEN** 收到 IntercomEvent::JoinRejected(reason)
- **THEN** page 切回 UngroupedHome + join_error = Some("Join rejected")，3s 后自动清空

### Requirement: GroupFormed 事件切页
host 端 pairing 三阶段完成所有 peer 加入后，`IntercomEvent::GroupFormed { host_id, peers }` SHALL 触发 IntercomApp 切 page=Main + 初始化 peers Vec。

#### Scenario: host 端 GroupFormed
- **WHEN** host 收到所有 peer 加入完成，GroupFormed 事件投递
- **THEN** page 从 CreatingHost 切到 Main，peers 初始化，UI 显示主对讲页 + peer 卡片

### Requirement: hit_test 命中目标扩展
未组网页 SHALL 返回新 HitTarget 变体：`CreateHostButton` / `SearchHostsButton` / `HostListItem(usize)` / `JoinButton` / `RefreshButton` / `CancelButton`。各按钮命中区域 SHALL 与 draw 布局一致。命中区域外 SHALL 返回 None。

#### Scenario: CreateHostButton 命中
- **WHEN** 触摸坐标 (120, 110) 落在 Create Group 按钮区域
- **THEN** hit_test 返回 Some(HitTarget::CreateHostButton)，dispatch 转成 pairing_action=StartHost
