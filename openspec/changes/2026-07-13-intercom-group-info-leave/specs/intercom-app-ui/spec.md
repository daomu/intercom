## MODIFIED Requirements

### Requirement: GroupInfo 页 Leave Group 按钮
`IntercomPage::GroupInfo` 底部 SHALL 显示 "Leave Group" 按钮。点击触发 `confirming_leave = true` 显示二次确认模态。模态 SHALL 半透明遮罩覆盖全屏（alpha=128），下层 GroupInfo 内容仍可见但 dim。模态中央显示警告卡片："Leave Group?" + "将失去所有 peer" + Cancel / Confirm 两按钮。

#### Scenario: 点击 Leave Group 弹出模态
- **WHEN** 用户在 GroupInfo 页点击底部 Leave Group 按钮
- **THEN** hit_test 命中 LeaveGroupButton，confirming_leave=true，下一 tick 渲染半透明遮罩 + 警告卡片

#### Scenario: 模态期间其他触摸被遮罩拦截
- **WHEN** confirming_leave=true，用户触摸非 Confirm/Cancel 区域
- **THEN** hit_test 返回 None（被遮罩拦截），dispatch 不触发任何 action，模态保持显示

### Requirement: 二次确认路由到 service
confirming_leave=true 时模态 Confirm 按钮命中 SHALL 返回 `IntercomAction::LeaveGroup` + `confirming_leave=false`。controller SHALL 把 LeaveGroup action 转成 `intercom_svc.leave_group()` 调用，service 返回的 `Vec<IntercomEvent>`（含 `LeftGroup`）经 ui_q 投递回主循环。Cancel 按钮命中 SHALL 仅设 `confirming_leave=false`，不触发 service 调用。

#### Scenario: 确认退出触发 service
- **WHEN** 用户在模态点击 Confirm
- **THEN** dispatch 返回 `[LeaveGroup]`，controller 调 `intercom_svc.leave_group()`，service 清 tracker + 广播 LeftGroup 包 + 清 NVS，返回事件投递 ui_q

#### Scenario: 取消保持群组
- **WHEN** 用户在模态点击 Cancel
- **THEN** confirming_leave=false，模态关闭，GroupInfo 页继续显示，群组状态不变

### Requirement: LeftGroup 事件切回未组网
`on_intercom_event(LeftGroup)` SHALL 切 page=UngroupedHome + 清空 peers Vec + confirming_leave=false。本端主动 leave 与远端 host leave（收到 LeftGroup 广播） SHALL 共用同一切页路径。

#### Scenario: 本端主动离开
- **WHEN** 本端调 leave_group() 收到 LeftGroup 事件
- **THEN** page 从 GroupInfo 切到 UngroupedHome，peers.clear()，confirming_leave=false，下一 tick 渲染未组网主入口页

#### Scenario: 远端 host 离开本端被通知
- **WHEN** host 离开 group 广播 LeftGroup，本端 peer 收到
- **THEN** 同样 on_intercom_event(LeftGroup) 处理，切到 UngroupedHome，peers 清空，spec：peer SHALL 立即停止任何进行中的 PTT capture + 显示 toast "Host left"
