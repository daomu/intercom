## MODIFIED Requirements

### Requirement: EspNowNetworkService 实例化与 recv callback
`src/main.rs` SHALL 构造 `EspNowNetworkService` 实例并传入共享 `Arc<Mutex<VecDeque<NetworkEvent, 32>>>` 事件队列。`EspNowNetworkService::new()` SHALL 经 `esp_now_register_recv_cb` 注册接收回调；回调运行在 esp-idf esp_now task，SHALL 仅执行 `push_back` 投递（队列满则丢 + log::warn），SHALL NOT 在回调内调用任何 `Mutex` 长持有操作或 IntercomService 方法。

#### Scenario: 远端 peer 包到达经队列投递
- **WHEN** 远端 peer 发来 ESP-NOW 包，esp_now recv callback 触发
- **THEN** EspNowNetworkService 解析为 `RecvEvent{src, rssi, payload}`，`push_back(NetworkEvent::Recv(ev))`，主循环下一 tick drain 后调 `intercom_svc.on_recv(ev)`

#### Scenario: 队列满不阻塞 callback
- **WHEN** 事件队列已达 32 容量且 callback 继续触发
- **THEN** 新事件被丢弃 + log::warn，callback 立即返回，不阻塞 esp_now task

#### Scenario: callback 单次注册
- **WHEN** `EspNowNetworkService::new()` 被调用
- **THEN** `esp_now_register_recv_cb` 全局只注册一次；多次 new SHALL panic 或返回 Err 以防重复注册

### Requirement: IntercomService 实例化与 restore
`src/main.rs` SHALL 构造 `IntercomService` 实例并调 `restore_from_nvs(&storage)`。restore 成功 SHALL 把上次保存的 group state 恢复到 tracker，并返回 `Vec<IntercomEvent>`（含 GroupRestored 等）供 controller 投递到 UiEventQueue 触发 model 更新。restore 失败 SHALL log::error + 设备退回 ungrouped，SHALL NOT panic。

#### Scenario: 重启恢复上次 group
- **WHEN** 设备重启，NVS 中存有上次 group `{host_id, channel, peers}`
- **THEN** `restore_from_nvs()` 返回 `GroupRestored` 事件，main loop 投递到 ui_q，IntercomApp 切到 grouped 主对讲页显示已恢复的 peer 列表

#### Scenario: NVS 损坏退回 ungrouped
- **WHEN** NVS group 数据损坏，`restore_from_nvs()` 返回 Err
- **THEN** main loop log::error，设备停在 ungrouped 等待用户重新创建群组，SHALL NOT panic 或 brick

### Requirement: IntercomEvent 跨线程桥
`UiEvent::Intercom` SHALL 携带真实 `crate::intercom::IntercomEvent` 类型（替代 change 12 的 `()` 占位）。主循环每 tick SHALL：(1) drain `network_q` 调 `intercom_svc.on_recv/on_send_done` 把返回的 `Vec<IntercomEvent>` 投递 ui_q；(2) 调 `intercom_svc.tick()` 把返回的 `Vec<IntercomEvent>` 投递 ui_q；(3) drain ui_q 对 `UiEvent::Intercom(e)` 调 `intercom_app.on_intercom_event(e)`。

#### Scenario: peer 上线刷新 UI
- **WHEN** esp_now recv callback 收到 PeerOnline 包，经 on_recv 投递 ui_q
- **THEN** 主循环 drain 后调 `intercom_app.on_intercom_event(PeerOnline{id, rssi_4})`，IntercomApp 切到 grouped 页 + 新 peer 卡片显示，dirty=true 触发重绘

#### Scenario: heartbeat tick 投递 GroupFormed
- **WHEN** host 端 pairing 三阶段完成，所有 peer 已 join
- **THEN** `intercom_svc.tick()` 返回 `GroupFormed{host_id, peers}`，ui_q 投递后 IntercomApp 切到主对讲页

### Requirement: pairing 入口路由
`IntercomApp.dispatch()` 在未组网页触发 create host / search hosts / join 时 SHALL 在 `IntercomAppOutcome.pairing_action` 字段返回 `PairingAction` 枚举。controller SHALL 把 `PairingAction::StartHost` 等转成 `intercom_svc.set_state(IntercomState::Hosting/Joining)` 调用，启动 pairing 三阶段流程（change 09）。

#### Scenario: 用户点 create host
- **WHEN** 用户在 ungrouped 页点击"创建群组"按钮
- **THEN** `dispatch` 返回 `pairing_action: Some(StartHost)`，controller 调 `intercom_svc.set_state(IntercomState::Hosting(HostPhase::Advertising))`，host 开始广播

#### Scenario: 用户取消搜索
- **WHEN** 用户在 searching 阶段按 back 退出
- **THEN** `dispatch` 返回 `pairing_action: Some(Cancel)`，controller 调 `intercom_svc.set_state(IntercomState::Idle)`，停止扫描
