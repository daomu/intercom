> **实现适配说明（Adapt to real code）**：本变更按 spec 意图实现，签名适配实际代码：
> - 实际无 `IntercomAction` 枚举 / `IntercomAppOutcome.actions` 字段。改用与 ungrouped-ui 一致的 **HitTarget + tap 方法** 模式：view `hit_test` 返回 `LeaveGroupButton/ConfirmLeaveButton/CancelLeaveButton`，controller 路由到 `IntercomApp::tap_leave_group/tap_confirm_leave/tap_cancel_leave`。（对应原 1.2/1.4）
> - 实际 `IntercomService::leave_group()` 仅 `tracker.stop()`（返回 `()`，不清 NVS、不广播）。故清 NVS group + 投递 `LeftGroup` 事件在 `main.rs` controller 手动完成。（对应 4.1/4.2）
> - Rgb565 无 alpha 通道，遮罩 alpha=128 用纯色暗化近似（`draw_leave_modal`）。（对应 2.3）
> - 构建 / `cargo test` / 双设备任务保留未勾选，待在带 ESP-IDF 工具链的机器上验证。

## 1. confirming_leave 子状态

- [x] 1.1 在 `src/apps/intercom_app.rs` IntercomApp 新增 `confirming_leave: bool` 字段（默认 false）
- [x] 1.2 ~~IntercomAppOutcome 新增 actions~~ → 适配为 `tap_*` 方法：`tap_confirm_leave()` 返回 `bool`（true=已确认，controller 执行 leave_group）
- [x] 1.3 dispatch hit_test 命中 LeaveGroupButton → `tap_leave_group()` 置 `confirming_leave = true`
- [x] 1.4 命中 ConfirmLeaveButton → `tap_confirm_leave()` 置 `confirming_leave = false` + 返回 true
- [x] 1.5 命中 CancelLeaveButton → `tap_cancel_leave()` 置 `confirming_leave = false`

## 2. GroupInfo view 扩展

- [x] 2.1 在 `src/apps/view/intercom_view.rs` `draw_group_info_page` 底部新增 Leave Group 按钮区域
- [x] 2.2 当 `confirming_leave=true` 时叠加遮罩 + 居中模态卡片（"Leave Group?" + "Lose all peers." + Cancel/Confirm 按钮）
- [x] 2.3 遮罩用纯色暗化近似（Rgb565 无 alpha 通道，`draw_leave_modal`）

## 3. hit_test 扩展

- [x] 3.1 LeaveGroupButton 命中区域（底部，`LEAVE_BTN_*` 常量）
- [x] 3.2 confirming_leave=true 时 hit_test 仅返回 ConfirmLeaveButton / CancelLeaveButton 命中
- [x] 3.3 confirming_leave=true 时 GroupInfo 原按钮命中目标返回 None（被遮罩拦截）

## 4. controller 路由

- [x] 4.1 在 `src/main.rs` 命中 ConfirmLeaveButton → `intercom_svc.leave_group()` + 投递 `LeftGroup` 到 ui_queue + `storage.clear_group()`
- [x] 4.2 ~~验证 leave_group 清 tracker + 广播 + 清 NVS~~ → 实际 leave_group 仅停 tracker；广播/清 NVS 由 controller 手动补齐（见适配说明）

## 5. LeftGroup 事件处理

- [x] 5.1 在 `src/apps/intercom_app.rs` `on_intercom_event` 处理 `LeftGroup`：切 page=UngroupedHome + 清 peers + confirming_leave=false
- [x] 5.2 主循环 drain `UiEvent::Intercom(LeftGroup)` 调 `on_intercom_event` + 置 `group = None`（翻转 is_grouped）

## 6. 构建验证

- [ ] 6.1 `cargo build` 通过
- [ ] 6.2 `cargo test --lib` 通过（confirming_leave 状态机单元测试）
- [x] 6.3 单元测试：`leave_group_confirm_flow`（Leave→模态→Confirm→返回 true→LeftGroup 切回 UngroupedHome）+ `leave_group_cancel_keeps_group`（Cancel 路径）
- [ ] 6.4 双设备 `cargo run`：A 在 group 中 → GroupInfo → Leave → 确认 → A 切回 UngroupedHome；B 收到 LeftGroup 也切回（需 change #2 #5 完成）
