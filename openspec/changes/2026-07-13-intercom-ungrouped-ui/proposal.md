## Why

`IntercomPage` 枚举当前只有 `Main` / `VoiceChanger` / `GroupInfo` 三页，`draw_not_grouped()` 只是 placeholder 文本 "No Group / Use Settings to create or join"——spec `intercom-app-ui` 要求未组网 UI 必须支持完整 create-host / search-hosts / join-flow 三阶段流程（spec §3.x ungrouped flow），但代码完全缺失。用户确认"未组网 UI 要实现的"。

当前 ungrouped 状态下用户除了回 Launcher 没有任何路径创建或加入群组，对讲机功能完全无法启动。本期补齐：

1. **未组网主入口页**：显示"创建群组" / "搜索群组"两个大按钮 + 简短说明（"你是 Host 时其他人可加入你；Join 时需 Host 在线"）
2. **创建群组流程页**：点击 create host → 显示"正在广播..." + spinner + 等待 peer 加入；显示已加入 peer 数量；点 back 取消回到主入口
3. **搜索群组流程页**：点击 search → 显示发现的 host 列表（按 rssi 排序）+ "刷新" + 选中一个 → "加入" 按钮
4. **加入群组确认页**：选中 host 后显示 host 信息（ID 简称 / rssi / 信号格数）+ "加入" / "取消"
5. **pairing.rs 三阶段集成**：UI 状态切换与 `pairing::start_host` / `search_hosts` / `join_host` 集成（pairing 逻辑已建好，change #2 已路由 pairing_action 到 intercom_svc.set_state）

## What Changes

- **修改 `src/apps/intercom_app.rs`**：`IntercomPage` 枚举新增 `UngroupedHome` / `CreatingHost` / `SearchingHosts` / `JoiningHost(HostId)` 变体；未组网时默认 `UngroupedHome`
- **修改 `src/apps/view/intercom_view.rs`**：新增 `draw_ungrouped_home` / `draw_creating_host` / `draw_searching_hosts` / `draw_joining_host` 四个 view 函数；替换原 `draw_not_grouped` placeholder
- **修改 `src/apps/view/intercom_view.rs`** `hit_test`：未组网页各按钮命中目标返回 `HitTarget::CreateHostButton` / `SearchHostsButton` / `JoinHostButton(host_idx)` / `CancelButton` / `RefreshButton`
- **修改 `src/apps/intercom_app.rs`** `dispatch`：未组网页触摸命中按钮 → 返回 `pairing_action`（change #2 已建路由），同时切 page 到对应流程页
- **修改 `src/apps/intercom_app.rs`** `on_intercom_event`：处理 `HostDiscovered(host_id, rssi)` → 更新 `discovered_hosts: Vec<HostInfo>`；`JoinAccepted` → 切到 grouped Main 页；`JoinRejected` → 切回 UngroupedHome + 显示错误

## Capabilities

### Modified Capabilities
- `intercom-app-ui`: 未组网 UI SHALL 完整实现 create-host / search-hosts / join-flow 三阶段流程，SHALL NOT 仅显示 placeholder 文本
