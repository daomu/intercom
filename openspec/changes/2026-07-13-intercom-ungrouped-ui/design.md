## Context

`IntercomPage` 枚举（`src/apps/intercom_app.rs`）当前只有 `Main` / `VoiceChanger` / `GroupInfo`。spec `intercom-app-ui` §3 明确要求未组网时有独立主入口 + 创建/搜索/加入三流程页。当前 `draw_not_grouped`（`src/apps/view/intercom_view.rs`）只是 placeholder 文本。

pairing 三阶段逻辑（`src/intercom/pairing.rs`，change 09）已建好：`start_host` / `search_hosts` / `join_host` / `on_pairing_event`。change #2 已把 `pairing_action` 从 controller 路由到 `intercom_svc.set_state`，pairing 事件经 `IntercomEvent::HostDiscovered` / `JoinAccepted` / `JoinRejected` 投递到主循环 → IntercomApp.on_intercom_event。

用户决策："未组网 UI 要实现的"。本期补齐 UI 层 + IntercomApp 状态机扩展。

## Goals / Non-Goals

**Goals:**
- 4 个新 IntercomPage 变体 + 4 个新 view draw 函数 + hit_test 命中目标
- create host 触发 PairingAction::StartHost → 显示"广播中" + peer 加入数
- search hosts 触发 PairingAction::SearchHosts → 显示发现的 host 列表
- join host 触发 PairingAction::Join(host_id) → 显示 host 信息 + 确认
- HostDiscovered/JoinAccepted/JoinRejected 事件正确刷新 UI
- 选中 host 后"加入"按钮可触发；back 在 searching 阶段可取消

**Non-Goals:**
- pairing 协议本身（change 09 已建好）
- 群组信息页 leave group + 二次确认（change #6 范围）
- 变声器 UI（change #7 范围）
- PTT 区交互（change #4 范围，但 PTT 区在未组网页不显示，只在 grouped Main 显示）

## Design

### IntercomPage 扩展

```
pub enum IntercomPage {
    UngroupedHome,        // 新
    CreatingHost,         // 新
    SearchingHosts,       // 新
    JoiningHost(HostId),  // 新
    Main,                 // grouped 主对讲页
    VoiceChanger,
    GroupInfo,
}
```

未组网时 `intercom_app.page = UngroupedHome`；grouped 后切 `Main`。

### UngroupedHome 布局

```
┌────────────────────────────┐
│       No Group Yet          │  标题
│                              │
│  ┌──────────────────────┐   │
│  │   Create Group       │   │  大按钮 1
│  │   (Become Host)      │   │
│  └──────────────────────┘   │
│                              │
│  ┌──────────────────────┐   │
│  │   Search Groups      │   │  大按钮 2
│  │   (Join Existing)     │   │
│  └──────────────────────┘   │
│                              │
│  Tip: Host 在线时其他可加入 │  说明文字
└────────────────────────────┘
```

hit_test：CreateHostButton 区域 (x∈[20,220], y∈[80,140])，SearchHostsButton (x∈[20,220], y∈[160,220])。

### CreatingHost 布局

```
┌────────────────────────────┐
│  Creating Group...          │
│                              │
│         [ spinner ]          │  旋转动画
│                              │
│   Broadcasting...            │
│                              │
│   Peers joined: 0            │  实时计数
│                              │
│  [ Back ]                    │  取消
└────────────────────────────┘
```

back → `pairing_action = Cancel` → `intercom_svc.set_state(Idle)` + 切回 UngroupedHome。

### SearchingHosts 布局

```
┌────────────────────────────┐
│  Searching Groups...        │
│                              │
│  ┌─ Host: ABC-12 [4 bars] ┐ │  rssi 排序
│  ├─ Host: XYZ-34 [3 bars] ┤ │  列表
│  └─ Host: DEF-56 [2 bars] ┘ │
│                              │
│  [ Refresh ]   [ Join ]      │  选中后 Join 高亮
│                              │
│  [ Back ]                    │
└────────────────────────────┘
```

`HostDiscovered(host_id, rssi)` → `discovered_hosts.push(HostInfo{...})` + 按 rssi 排序。点击列表项选中（`selected_host: Option<usize>`），"Join" 按钮触发 `PairingAction::Join(host_id)`。

### JoiningHost 布局

```
┌────────────────────────────┐
│  Joining ABC-12...          │
│                              │
│   Waiting for approval      │
│                              │
│         [ spinner ]          │
│                              │
│  [ Cancel ]                  │
└────────────────────────────┘
```

`JoinAccepted` → 切到 Main grouped 页（spec 要求：加入成功立即进入主对讲页）。
`JoinRejected` → 切回 UngroupedHome + 显示 toast "Join rejected, try another host"。

### IntercomApp 字段扩展

```
pub struct IntercomApp {
    page: IntercomPage,
    ptt: VoicePttMachine,
    mode: IntercomMode,
    peers: Vec<PeerCard>,
    channel_busy: bool,
    // 新增：
    discovered_hosts: Vec<HostInfo>,
    selected_host: Option<usize>,
    join_error: Option<&'static str>,  // toast 显示用
    creating_peer_count: u32,           // CreatingHost 页显示已加入数
}
```

### on_intercom_event 扩展

```
match ev {
    IntercomEvent::HostDiscovered(host_id, rssi) => {
        if self.page == SearchingHosts {
            self.discovered_hosts.push(HostInfo{host_id, rssi});
            // 按 rssi 排序降序
            self.discovered_hosts.sort_by(|a, b| b.rssi.cmp(&a.rssi));
        }
        vec![]
    }
    IntercomEvent::PeerJoined(peer_id) => {
        if self.page == CreatingHost {
            self.creating_peer_count += 1;
        }
        vec![]
    }
    IntercomEvent::JoinAccepted => {
        self.page = IntercomPage::Main;
        vec![]
    }
    IntercomEvent::JoinRejected(reason) => {
        self.page = IntercomPage::UngroupedHome;
        self.join_error = Some("Join rejected");
        vec![]
    }
    IntercomEvent::GroupFormed { host_id, peers } => {
        self.page = IntercomPage::Main;
        self.peers = peers.iter().map(|p| PeerCard::from(p)).collect();
        vec![]
    }
    _ => vec![],
}
```

## Risks

- discovered_hosts 列表在 long searching 后可能很长，限制最大 8 项（超出 log::warn 丢旧的）
- spinner 动画需要每 100ms 刷新一次 frame，但主循环 500ms tick——通过 dirty flag + tick 计数模拟动画，或降低 spinner 复杂度（静态文字 "Broadcasting..." + 一个圆点闪烁）
- join_error toast 需 3s 后自动清，避免永久显示

## Dependencies

- 前置：`2026-07-13-wire-network-runtime`（pairing_action 路由 + IntercomEvent 投递）
- 不阻塞：与 #6 #7 可并行
