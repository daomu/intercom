## Context

`IntercomPage::GroupInfo`（change 13）已有 `draw_group_info` 渲染 peer 列表 + group meta（host_id / channel / members 数）。但 spec §8 要求 group info 页有"退出群组"入口 + 二次确认（防误触）。`IntercomService::leave_group()` 方法（change 08）已建好但从未被调用。

用户加入 group 后唯一退出方式是重启设备，不友好。本期补 leave group UI + wiring。

## Goals / Non-Goals

**Goals:**
- GroupInfo 页底部新增 Leave Group 按钮
- 二次确认模态（半透明遮罩 + 居中卡片 + Confirm/Cancel 按钮）
- LeaveGroup 路由到 `intercom_svc.leave_group()`，service 清 tracker + 发 LeftGroup 广播包 + 清 NVS
- LeftGroup 事件切回 UngroupedHome + 清 peers
- peer 端 host 离开时也收到通知并切回 UngroupedHome

**Non-Goals:**
- group info 页其他扩展（peer 详细信息 / 邀请新 peer / 转让 host 角色）
- 群组创建后改名 / 改 channel（spec 未要求）
- 多 group 切换（spec 单 group）

## Design

### GroupInfo 布局（含 Leave Group）

```
┌────────────────────────────┐
│  Group Info                 │
│                              │
│  Host: ABC-12 (You)          │
│  Channel: 5                  │
│  Members: 3                  │
│                              │
│  Peers:                      │
│   - XYZ-34 [4 bars]          │
│   - DEF-56 [3 bars]          │
│                              │
│  ┌──────────────────────┐   │
│  │   Leave Group         │   │  底部按钮
│  └──────────────────────┘   │
└────────────────────────────┘
```

### 二次确认模态（confirming_leave=true 时叠加）

```
       ┌──────────────────┐
       │ Leave Group?      │  半透明遮罩 + 居中卡片
       │                   │
       │ 将失去所有 peer   │
       │                   │
       │ [Cancel] [Confirm] │
       └──────────────────┘
```

遮罩吃所有触摸事件，只 Confirm/Cancel 两按钮可命中。

### dispatch 路由

```
match hit_target {
    HitTarget::LeaveGroupButton => {
        self.confirming_leave = true;
        vec![]
    }
    HitTarget::ConfirmLeaveButton => {
        self.confirming_leave = false;
        vec![IntercomAction::LeaveGroup]  // 经 controller 调 intercom_svc.leave_group()
    }
    HitTarget::CancelLeaveButton => {
        self.confirming_leave = false;
        vec![]
    }
    _ => vec![],
}
```

### leave_group service 调用

controller:
```
if outcome.contains(&IntercomAction::LeaveGroup) {
    let evs = intercom_svc.leave_group();
    for ev in evs { ui_q.push_back(UiEvent::Intercom(ev)); }
}
```

`leave_group()` 清 tracker + 广播 LeftGroup 包 + 清 NVS group state，返回 `Vec<IntercomEvent>` 含 `LeftGroup`。

### on_intercom_event 处理

```
IntercomEvent::LeftGroup => {
    self.page = IntercomPage::UngroupedHome;
    self.peers.clear();
    self.confirming_leave = false;
    vec![]
}
```

### peer 端 host 离开

host 主动 leave 时广播 LeftGroup 包；peer 收到后同 LeftGroup 事件处理切回 UngroupedHome。

## Risks

- leave_group 期间 PTT 还在 talking 状态：先 stop_capture 再 leave（在 service 内部处理）
- 网络丢包导致 LeftGroup 广播未达：peer 端通过 heartbeat timeout 检测 host 离线，独立触发 LeftGroup（change 08 已建好 heartbeat timeout）
- 二次确认模态遮罩吃所有触摸：在 confirming_leave=true 时 hit_test 其他区域返回 None（被遮罩拦截），但下层的 Leave Group 按钮视觉仍可见（半透明遮罩上层）

## Dependencies

- 前置：`2026-07-13-wire-network-runtime`（intercom_svc.leave_group 可用）
- 前置：`2026-07-13-intercom-ungrouped-ui`（LeftGroup 切回 UngroupedHome 需要该页存在）
- 可与 #7 并行
