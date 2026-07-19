## Why

`EspNowNetworkService`（change 14）+ `IntercomService` orchestrator（change 08 heartbeat）作为纯逻辑 / 半成品已建好，但 main.rs 从未构造它们。当前网络栈处于"代码全有但未接线"状态：

1. **`EspNowNetworkService` 未实例化**：`RadioDriver::init()` 只调用了 `EspNow` 的初始化，但从未 `EspNowNetworkService::new()`，也没有 set_channel/add_peer 调用。ESP-NOW send/recv 路径完全空转。
2. **ESP-NOW recv 回调未注册**：`EspNowNetworkService` 内部有 recv 逻辑，但没有把 `on_recv` callback 注册到 `esp_now_register_recv_cb`，远端 peer 的包收不到。
3. **`IntercomService` orchestrator 未实例化**：`src/intercom/heartbeat.rs` 定义了 `IntercomService` struct 含 tracker + sink + restore_from_nvs，但 main.rs 从未 `IntercomService::new()`。heartbeat tick / `set_state` / `on_recv` / `leave_group` 全部从未被调用，设备永远停在"未组网"状态。
4. **`IntercomEvent` 桥接缺失**：跨线程 ESP-NOW recv callback → 主线程 model 的投递机制未建。`UiEvent::Intercom(())` 占位变体需要替换为真实 `IntercomEvent` 类型，并接通 drain 路径。
5. **pairing 三阶段流程（change 09）从未触发**：`pairing::start_host()` / `search_hosts()` / `join_host()` 逻辑存在但无人调用，未组网 UI（change #5）依赖这些入口。

本期补齐：(a) `EspNowNetworkService` 实例化 + recv callback；(b) `IntercomService` 实例化 + heartbeat tick；(c) 跨线程 `Mutex<VecDeque<IntercomEvent>>` 桥；(d) pairing 入口从 IntercomApp 路由到 `IntercomService`。

## What Changes

- **修改 `src/main.rs`**：构造 `EspNowNetworkService::new(radio.esp_now)` + 注册 recv callback → `Arc<Mutex<VecDeque<NetworkEvent>>>`；构造 `IntercomService::new(network_svc, storage.clone())` + 调 `restore_from_nvs()`；每 500ms tick 调 `intercom_svc.tick()` drain → 投递 `IntercomEvent` 到 `UiEventQueue`；主循环 drain `UiEvent::Intercom` 调 `intercom_app.on_event()` + `intercom_svc.on_recv()`
- **修改 `src/services/network/mod.rs`**：`EspNowNetworkService::new()` SHALL 接收 `EspNow` 句柄 + `Arc<Mutex<VecDeque<NetworkEvent>>>` 事件队列；recv callback SHALL `push_back(NetworkEvent::Recv(RecvEvent))`；队列满时丢 + log::warn 不阻塞 callback
- **修改 `src/intercom/heartbeat.rs`**：`IntercomService::tick()` SHALL 返回 `Vec<IntercomEvent>`（peer 上线/离线/状态变更/group 形成等），供 controller 投递到 UiEventQueue；`on_recv(RecvEvent)` 返回 `Vec<IntercomEvent>`
- **修改 `src/apps/intercom_app.rs`**：新增 `on_intercom_event(&mut self, ev: IntercomEvent) -> Vec<VoiceAction>` 入口（与现有 `dispatch` 分离，专门吃 IntercomEvent）；UI 状态机根据 PeerOnline/PeerOffline/GroupFormed/TalkState 切换 ui_state
- **修改 `src/apps/mod.rs`**：`UiEvent::Intercom` 占位类型 `()` 替换为真实 `crate::intercom::IntercomEvent`
- **修改 `src/hal/radio.rs`**：`RadioDriver` SHALL 暴露 `esp_now()` 访问器供 `EspNowNetworkService` 取得句柄；若已存在仅 wiring

## Capabilities

### Modified Capabilities
- `network-service`: EspNowNetworkService SHALL 真正实例化；recv callback SHALL 注册并经事件队列投递
- `app-shell`: 主循环 SHALL drain 跨线程 NetworkEvent + IntercomEvent 并派发到 IntercomService / IntercomApp；UiEvent::Intercom 占位类型 SHALL 替换为真实类型
