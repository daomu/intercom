## 1. NetworkService 实例化 + recv callback

- [x] 1.1 在 `src/hal/radio.rs` 暴露 `RadioDriver::esp_now(&self) -> &EspNow`（或 `take_esp_now(self)` 移交所有权）— 已存在 `take_espnow()` + `espnow()`
- [x] 1.2 在 `src/services/network/mod.rs` 修改 `EspNowNetworkService::new(esp_now: EspNow, event_q: Arc<Mutex<VecDeque<NetworkEvent, 32>>>) -> Result<Self, NetworkError>` — 适配实际代码：`new(espnow)` 接收句柄，队列经 main.rs `on_recv(cb)` 注册（callback 内 push 到队列）
- [x] 1.3 recv callback：解析 esp_now_recv_info + payload → `RecvEvent{src, rssi, payload}`，`event_q.push_back(NetworkEvent::Recv(ev))`，队列满 log::warn 丢不阻塞 — main.rs 注册 callback → `push_network_event`（drop-on-full）
- [ ] 1.4 send callback 同理处理 send 状态 → `NetworkEvent::SendDone(status)` — 延后：`SendDone` 变体已定义但 esp-idf-svc send-status 回调尚未注册（无消费方，PTT/audio 变更再接）
- [x] 1.5 单元测试：mock event_q 验证 callback 正确 push；队列满验证丢不 panic — `network_queue_drops_when_full` 覆盖队列满丢弃

## 2. IntercomService 实例化 + restore

- [x] 2.1 在 `src/intercom/heartbeat.rs` 确认 `IntercomService::new(network_svc, storage) -> Self` 签名；`restore_from_nvs(&storage) -> Result<Vec<IntercomEvent>, IntercomError>` — 适配实际代码：`new()` + `set_heartbeat_sink()` + `restore_from_nvs(now_ms, Option<&GroupInfo>) -> RestoreOutcome`
- [x] 2.2 在 `src/main.rs` 构造 `EspNowNetworkService` + `IntercomService`，调 `restore_from_nvs()` 把返回的 IntercomEvent 投递到 ui_q — 构造 + 注册 recv callback + set_heartbeat_sink + restore + add_peer
- [x] 2.3 `restore_from_nvs` 失败 log::error + 设备退回 ungrouped（不 panic）— `RestoreOutcome::NoGroup` 分支

## 3. 主循环 drain + 派发

- [x] 3.1 在 `src/apps/mod.rs` 把 `UiEvent::Intercom(())` 替换为 `UiEvent::Intercom(crate::intercom::IntercomEvent)` — 已是真实类型
- [x] 3.2 在 `src/apps/intercom_app.rs` 新增 `on_intercom_event(&mut self, ev: IntercomEvent) -> Vec<VoiceAction>`，按 PeerOnline/PeerOffline/GroupFormed/TalkState/LeftGroup 切换 ui_state + page
- [x] 3.3 在 `src/main.rs` 主循环：drain `network_q` → `intercom_svc.on_recv(ev)` 把返回的 IntercomEvent 投递 ui_q — peer 上线 → `PeerListChanged`
- [x] 3.4 主循环：`intercom_svc.tick()` 每 500ms 调用，返回的 IntercomEvent 投递 ui_q — 离线扫描 → `PeerListChanged`
- [x] 3.5 主循环：drain `ui_q` 处理 `UiEvent::Intercom(e)` 调 `intercom_app.on_intercom_event(e)` 收集 VoiceAction（change #4 实际执行）

## 4. pairing 入口路由

- [x] 4.1 在 `src/apps/intercom_app.rs` `IntercomAppOutcome` 增 `pairing_action: Option<PairingAction>` 字段
- [x] 4.2 定义 `PairingAction { StartHost, SearchHosts, Join(HostId), Cancel }` — `Join([u8; 6])`（MAC）
- [x] 4.3 在 `src/main.rs` 处理 `pairing_action`：调 `intercom_svc.set_state(IntercomState::Hosting/Joining/...)` — `route_pairing_action()`（实际 phase：Hosting(Discovering)/Joining(Searching/Requesting)）
- [x] 4.4 取消 pairing（用户按 back 在 searching 阶段）→ `intercom_svc.set_state(IntercomState::Idle)`

## 5. 构建验证（需在用户机器执行）

- [ ] 5.1 `cargo build` 通过
- [ ] 5.2 `cargo test --lib` 通过
- [ ] 5.3 `cargo run` 验证：启动设备日志显示 EspNowNetworkService init OK + IntercomService restore OK；无 group 时停在 ungrouped（需硬件）
- [ ] 5.4 双设备验证：一台 create host，另一台 search 看到 peer（需 2 台硬件）
