> **实现说明（适配实际代码）**：设计稿假设的若干类型/事件签名与实际代码不符，已按 spec 意图适配实际 API：
> - `IntercomPage::JoiningHost` 采用无载荷变体（实际无 `HostId` 类型；选中项由 `selected_host: Option<usize>` 跟踪）。
> - `discovered_hosts` 采用实际的 `pairing::DiscoveredHost`（携带完整 `[u8;6]` MAC，`Join` 需要）；`join_error` 用 `Option<String>`（`JoinRejected(String)` 携带原因）。
> - `HostInfo` 复用 `pairing::HostInfo`（`from_discovered` 生成 name/mac_suffix4/信号格），未新造设计稿里的虚构类型。
> - 实际无 `PeerJoined` 事件 → CreatingHost 页用 `PeerOnline` 递增 `creating_peer_count`。
> - 实际 `HostDiscovered(usize)` 只携带计数、`GroupFormed` 无字段 → host 列表由 coordinator 通过 `set_discovered_hosts` 推送（排序在 pairing 层）；`GroupFormed`/`JoinAccepted` → Main。
> - 触摸按钮走新增的 `HitTarget`（`CreateHostButton`/`SearchHostsButton`/`HostListItem{idx}`/`JoinSelectedButton`/`RefreshHostsButton`/`PairingBackButton`）+ `IntercomApp::tap_*` 方法，由 `main.rs` 路由到 `route_pairing_action`。
> - 构建/双机验证任务（8.1/8.2/8.4）保留未勾选，待用户在带 ESP-IDF 工具链的机器上执行。

## 1. IntercomPage 扩展

- [x] 1.1 在 `src/apps/intercom_app.rs` `IntercomPage` 枚举新增 `UngroupedHome` / `CreatingHost` / `SearchingHosts` / `JoiningHost` 变体（适配：JoiningHost 无载荷）
- [x] 1.2 `IntercomApp::new()` 默认 `page = UngroupedHome`（spec：未组网时默认入口）
- [x] 1.3 新增字段：`discovered_hosts: Vec<DiscoveredHost>` / `selected_host: Option<usize>` / `join_error: Option<String>` / `creating_peer_count: u32`
- [x] 1.4 复用 `pairing::HostInfo`（`from_discovered` 生成 name/mac_suffix4/rssi_4bar）作为 UI 数据契约

## 2. UngroupedHome view

- [x] 2.1 在 `src/apps/view/intercom_view.rs` 新增 `draw_ungrouped_home(fb, app)` 函数：标题 + Create Group 按钮 + Search Groups 按钮 + 说明文字
- [x] 2.2 hit_test：CreateHostButton / SearchHostsButton 命中区域（几何常量 UG_BTN_*）
- [x] 2.3 命中 CreateHostButton → `tap_create_host()` 返回 `StartHost` + 切 page=CreatingHost
- [x] 2.4 命中 SearchHostsButton → `tap_search_hosts()` 返回 `SearchHosts` + 切 page=SearchingHosts + 清空 discovered_hosts

## 3. CreatingHost view

- [x] 3.1 新增 `draw_creating_host`：标题 + spinner（圆点闪烁基于 ctx.time_hms 秒数奇偶） + "Broadcasting..." + peer 计数 + Back 按钮
- [x] 3.2 hit_test 命中 Back → `tap_pairing_back()` 返回 `Cancel` + 切 page=UngroupedHome
- [x] 3.3 `on_intercom_event(PeerOnline)`（适配 PeerJoined）在 CreatingHost 页更新 creating_peer_count

## 4. SearchingHosts view

- [x] 4.1 新增 `draw_searching_hosts`：标题 + discovered_hosts 列表（最多 8 项，每项显示 host name 简称 + 信号格数 + 成员数） + Refresh + Join + Back 按钮
- [x] 4.2 hit_test：列表项命中 → `tap_host_item(idx)` 设 `selected_host = Some(idx)`；Join 按钮命中 + selected → `tap_join()` 返回 `Join(mac)` + 切 page=JoiningHost
- [x] 4.3 Refresh 按钮命中 → `tap_refresh()` 返回 `SearchHosts` + 清空 discovered_hosts 重新搜索
- [x] 4.4 `set_discovered_hosts()`（适配 HostDiscovered 仅计数）由 coordinator 从 pairing `host_list` 推送列表；排序在 pairing 层

## 5. JoiningHost view

- [x] 5.1 新增 `draw_joining_host`：标题 "Joining {host}" + "Waiting for approval" + Cancel 按钮
- [x] 5.2 hit_test Cancel → `tap_pairing_back()` 返回 `Cancel` + 切 page=UngroupedHome
- [x] 5.3 `on_intercom_event(JoinAccepted)` → 切 page=Main
- [x] 5.4 `on_intercom_event(JoinRejected(reason))` → 切 page=UngroupedHome + join_error = Some(reason)

## 6. GroupFormed 事件处理

- [x] 6.1 `on_intercom_event(GroupFormed)`（适配：无字段）→ 切 page=Main + 清 join_error/selected/discovered
- [x] 6.2 持久化 group state 到 NVS（由 IntercomService 处理，IntercomApp 仅 UI — 本期无 UI 侧动作）

## 7. join_error toast

- [x] 7.1 在 `draw_ungrouped_home` 顶部若 join_error.is_some() 显示红色 toast 文字
- [x] 7.2 主循环检测到 join_error 设置后启动 3s 定时器（join_error_deadline），到期 `clear_join_error()`

## 8. 构建验证

- [ ] 8.1 `cargo build` 通过
- [ ] 8.2 `cargo test --lib` 通过（IntercomApp 状态机扩展的单元测试）
- [x] 8.3 单元测试已编写：ungrouped_defaults_to_home / create_host_flow_to_grouped_main / search_select_join_flow / join_rejected_shows_error_and_returns_home / join_accepted_enters_main
- [ ] 8.4 双设备 `cargo run`：A create host → B search 看到 A → B join → 两端都进 grouped Main 页（需 change #2 #3 #4 完成）
