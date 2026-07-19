## Why

`IntercomPage::GroupInfo` 页当前 `draw_group_info` 渲染了 peer 列表 + group 信息，但缺少 spec 要求的"退出群组"按钮 + 二次确认模态（spec §8 leave group flow）。用户加入 group 后无法退出，只能重启设备退回 ungrouped。

`IntercomService::leave_group()` 方法（change 08 已建好）也未被任何 UI 路径调用。本期补齐 leave group UI + 路由到 service。

## What Changes

- **修改 `src/apps/intercom_app.rs`**：`IntercomPage::GroupInfo` 子状态新增 `confirming_leave: bool` 字段（是否显示二次确认模态）；dispatch 命中 LeaveGroupButton → 设 `confirming_leave = true`；命中 ConfirmLeave → 返回 `PairingAction::LeaveGroup`（或新增 `IntercomAction::LeaveGroup`）；命中 CancelLeave → `confirming_leave = false`
- **修改 `src/apps/view/intercom_view.rs`**：`draw_group_info` 新增底部"Leave Group"按钮；当 `confirming_leave=true` 时叠加半透明遮罩 + 居中确认模态（"确定退出群组？将失去所有 peer" + Confirm/Cancel 按钮）
- **修改 `src/apps/view/intercom_view.rs`** `hit_test`：LeaveGroupButton 命中区域；confirming_leave=true 时 ConfirmLeaveButton / CancelLeaveButton 命中区域
- **修改 `src/main.rs`**：处理 `IntercomAction::LeaveGroup` → 调 `intercom_svc.leave_group()`；service 返回 `LeftGroup` 事件经 ui_q 投递 → IntercomApp.on_intercom_event(LeftGroup) → 切 page=UngroupedHome + 清 peers

## Capabilities

### Modified Capabilities
- `intercom-app-ui`: GroupInfo 页 SHALL 显示 Leave Group 按钮 + 二次确认模态；LeaveGroup 事件 SHALL 切回 UngroupedHome
