## Context

`EspNowNetworkService`（`src/services/network/mod.rs`，change 14）实现了 `NetworkService` trait（init/set_channel/add_peer/send/recv），但 main.rs 从未实例化它。`RadioDriver::init()`（change 03）只调了 `EspWifi::new` + `EspNow::new`，把 `esp_now` 句柄留在内部未暴露。

`IntercomService`（`src/intercom/heartbeat.rs`，change 08）实现了 tracker + sink + restore_from_nvs + leave_group + set_state + on_recv + tick，是组网状态机的核心 orchestrator。但 main.rs 从未 `IntercomService::new()`，也未调 tick()。

`IntercomEvent` 枚举（change 13 intercom-state）已定义全部变体（PeerOnline/PeerOffline/GroupFormed/TalkState/VoicePacket/LeftGroup 等），但 `UiEvent::Intercom` 仍是 `()` 占位（change 12 遗留）。

跨线程桥的关键约束：ESP-NOW recv callback 运行在 esp-idf 的 esp_now task 上（不是主线程，也不是单独 std::thread），SHALL NOT 直接调 IntercomService（Mutex 阻塞 callback 风险），SHALL 经 `Arc<Mutex<VecDeque>>` 投递。

## Goals / Non-Goals

**Goals:**
- `EspNowNetworkService` 实例化 + recv callback 注册到 `esp_now_register_recv_cb`
- 跨线程 `Arc<Mutex<VecDeque<NetworkEvent>>>` 队列，recv callback push，主循环 drain
- `IntercomService` 实例化 + `restore_from_nvs()` 启动恢复 + 每 500ms `tick()`
- `IntercomEvent` 真实类型替换 `UiEvent::Intercom(())` 占位
- pairing 入口（host/search/join）从 IntercomApp.dispatch 经 controller 路由到 `IntercomService::set_state()`
- 主循环 drain `UiEvent::Intercom` 调 `intercom_app.on_intercom_event()` + `intercom_svc.on_recv()`（根据事件类型分派）

**Non-Goals:**
- 真音频包收发（`2026-07-13-wire-audio-pipeline` 范围）
- PTT 触发 voice packet 发送（`2026-07-13-wire-ptt-end-to-end` 范围）
- 未组网 UI 全流程（`2026-07-13-intercom-ungrouped-ui` 范围）
- 网络安全 / 加密（change 17 范围，本期不涉及）

## Design

### 跨线程事件桥

```
┌──────────────────────────────────────────────────────────────┐
│ esp_now_recv_cb (esp-idf task)                               │
│   │                                                          │
│   ▼                                                          │
│ EspNowNetworkService recv handler                            │
│   │ parse → RecvEvent{src, rssi, payload}                   │
│   ▼                                                          │
│ Arc<Mutex<VecDeque<NetworkEvent>>>.push_back(Recv(ev))       │
│   (满则丢 + log::warn，不阻塞 callback)                       │
└──────────────────────────────────────────────────────────────┘
                          │
                          ▼ (main loop 每 tick drain)
┌──────────────────────────────────────────────────────────────┐
│ main loop                                                    │
│   while let Some(ev) = network_q.lock().pop_front() {        │
│       let icom_evs = intercom_svc.on_recv(ev);               │
│       for e in icom_evs { ui_q.push_back(UiEvent::Intercom(e)); }│
│   }                                                          │
│   let icom_evs = intercom_svc.tick();                        │
│   for e in icom_evs { ui_q.push_back(UiEvent::Intercom(e)); }│
│   while let Some(ev) = ui_q.lock().pop_front() {             │
│       match ev {                                             │
│           UiEvent::Intercom(e) => {                          │
│               let actions = intercom_app.on_intercom_event(e);│
│               execute_voice_actions(actions); // change #4   │
│           }                                                   │
│           UiEvent::Network(n) => intercom_svc.on_recv(n),    │
│           UiEvent::Audio(_) => {} // change #3               │
│           UiEvent::Dirty => dirty = true,                    │
│       }                                                       │
│   }                                                          │
└──────────────────────────────────────────────────────────────┘
```

### 队列容量

- `network_q: VecDeque<NetworkEvent, 32>`（网络事件速率低，32 够）
- `ui_q: VecDeque<UiEvent, 64>`（已有，change 12）
- 队列满策略：丢 + log::warn，SHALL NOT 阻塞生产者

### IntercomService 实例化

```
let network_svc = EspNowNetworkService::new(
    radio.esp_now(),
    network_q.clone(),
)?;
let mut intercom_svc = IntercomService::new(
    network_svc,
    storage.clone(),
);
intercom_svc.restore_from_nvs(&storage)?;
```

`restore_from_nvs` 若有上次保存的 group，恢复 tracker 状态；否则设备停在 ungrouped。

### pairing 入口路由

IntercomApp 在未组网 UI 触发"创建群组"/"搜索群组"/"加入"时经 `dispatch()` 返回 `IntercomAppOutcome { pairing_action: PairingAction }`。controller 调：

```
match pairing_action {
    PairingAction::StartHost => intercom_svc.set_state(IntercomState::Hosting(HostPhase::Advertising)),
    PairingAction::SearchHosts => intercom_svc.set_state(IntercomState::Joining(JoinPhase::Scanning)),
    PairingAction::Join(host_id) => intercom_svc.set_state(IntercomState::Joining(JoinPhase::Joining(host_id))),
}
```

### 前置变更依赖

- `2026-07-12-ui-render-layer`（UiEvent 队列已建，占位需替换）
- `2026-07-13-wire-settings-side-effects`（无强依赖，但同 main.rs 改动建议先合并）

## Risks

- ESP-NOW recv callback 在 esp-idf task 上跑，SHALL NOT 持有任何 Mutex 超过 push_back 时间（< 1ms）
- `esp_now_register_recv_cb` 是全局单例，只能注册一次；若 EspNowNetworkService 多次 new 会冲突——构造 SHALL 只调一次
- WiFi init 会短暂断开 USB CDC，monitor 会断连——预期，文档说明
- `restore_from_nvs` 失败时设备 SHALL 退回 ungrouped，SHALL NOT panic

## Dependencies

- 前置：`2026-07-12-ui-render-layer`（UiEvent 队列）
- 阻塞：`2026-07-13-wire-audio-pipeline`（audio pipeline 依赖 network_svc.send）
- 阻塞：`2026-07-13-wire-ptt-end-to-end`（PTT VoiceAction 依赖 network_svc.send）
