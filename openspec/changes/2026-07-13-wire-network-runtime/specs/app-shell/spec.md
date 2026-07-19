## MODIFIED Requirements

### Requirement: 主循环跨线程事件 drain
主循环 SHALL 持有 `network_q: Arc<Mutex<VecDeque<NetworkEvent, 32>>>` + `ui_q: Arc<Mutex<VecDeque<UiEvent, 64>>>` 两个事件队列。每 tick SHALL：(1) drain network_q → intercom_svc.on_recv/on_send_done → 投递返回的 IntercomEvent 到 ui_q；(2) intercom_svc.tick() → 投递返回的 IntercomEvent 到 ui_q；(3) drain ui_q → 按 UiEvent 变体派发到 IntercomApp / IntercomService / dirty flag。SHALL NOT 在生产者（esp_now callback）侧直接调用任何 model 方法。

#### Scenario: 两队列依次 drain
- **WHEN** esp_now callback push 了 3 个 NetworkEvent 到 network_q，同时 ui_q 有 1 个 Pending
- **THEN** 主循环先 drain network_q 3 次调 on_recv 把产生的 IntercomEvent 投递 ui_q，再调 tick() 投递，最后 drain ui_q 全部派发

#### Scenario: 占位变体替换为真实类型
- **WHEN** `2026-07-12-ui-render-layer` 的 `UiEvent::Intercom(())` 占位被本 change 替换为 `UiEvent::Intercom(IntercomEvent)`
- **THEN** 主循环 drain 时 match 该变体 SHALL 调 `intercom_app.on_intercom_event(e)`，SHALL NOT 再记日志并忽略（占位行为）
