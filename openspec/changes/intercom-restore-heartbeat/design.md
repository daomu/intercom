## Context

前期变更已交付本变更所需的地基：
- change 03 `services-storage-crypto`：`StorageService::load_group() -> Option<GroupInfo>` / `clear_group()`；`CryptoService::derive_lmk(priv_key, peer_pub_key) -> [u8;16]`；`GroupInfo { my_priv_key, peers: Vec<PeerEntry>, mode, channel, schema_ver }`
- change 06 `services-network-espnow`：`NetworkService::init(channel: u8)` / `add_peer(mac: [u8;6], lmk: [u8;16])`；ESP-NOW 收发回调
- change 08 `intercom-packet-format`：HEARTBEAT 包（type=0x02，字段 seq/sender_id/state/mode）编解码
- change 09 `intercom-pairing-three-phase`：`IntercomState::Grouped(VoiceState)` 与组内 peer 表；`leave_group()` 已实现

当前缺口：上电后 `IntercomService::restore_from_nvs()` 仅是 trait 占位；无心跳任务；成员在线状态未维护；RSSI 仅在 ESP-NOW 收包回调里裸暴露，未做平滑/滞回。

技术设计 §6.4 / §19.3 给出冷启动恢复伪代码；§13.1/§13.2 给出心跳周期表与超时/信号规则；PRD §12.3 / §15 / §22.3 给出业务规则。

## Goals / Non-Goals

**Goals:**
- 冷启动时若 NVS 组信息有效：瞬时（无 RF 交互）恢复 `network.init(g.channel)` + 全套 LMK 加密表 + `Grouped(Idle)`
- 组信息损坏 / schema 不兼容 / `None`：`clear_group()` 后回 `Idle`（未组网），不卡死启动
- 心跳任务按状态相关周期发送 HEARTBEAT 包：待机 5s / 组队中 1s / 稳定语音 10s / 熄屏待机 10s
- 15s 无心跳/消息 → 标对端离线（保留成员表条目）；收到任意消息/心跳 → 立即标在线
- RSSI 4 格平滑 + 滞回，避免边界频繁跳变
- 去中心化 Peers 拓扑：每台设备独立维护成员状态表
- 心跳任务运行在 Task B，不新增 task

**Non-Goals:**
- 组队三阶段协议本身 → change 09
- 语音收发链路 → change 10/11
- UI 渲染成员卡片 / 信号条 → change 13
- 熄屏策略本身 → change 15（本变更仅暴露"熄屏待机时心跳周期 = 10s"接口供其切换）
- 心跳包内容加密强度升级（当前复用 ESP-NOW AES-CCM 链路层加密，不额外加应用层加密）
- RSSI 精确值显示（PRD §15.2 明确仅 4 格粗颗粒）

## Decisions

### D1：restore 路径零 RF 交互，直接 `init(g.channel)` 而非 ch1
技术设计 §6.4 / §19.3 明确：恢复时 `network.init(g.channel)` 直接到工作信道，不经发现信道 1。原因：组信息里已持久化协商好的工作信道，无需重新发现；冷启动时若先到 ch1 再切 ch 会引入不必要的 RF 占用与延迟。LMK 在内存里由 ECDH 重算后通过 `add_peer(mac, lmk)` 写入 ESP-NOW 硬件加密表，之后任意收发都走 AES-CCM 硬件加密，无空中明文。

### D2：组信息损坏 / schema 不兼容 → `clear_group()`，不回退默认值
PRD §12.3 / 技术设计 §7.3：组信息读取失败 / schema 不兼容时"直接清理（不回退默认）"。原因：组信息含密钥材料，无法安全回退默认；清理后回 `Idle`（未组网），用户可重新组队。系统设置（sys 命名空间）损坏才回退默认值——本变更不涉及。

### D3：心跳任务挂在 Task B 事件循环，不新增 task
三 task 模型（change 01 design.md D10）已固化：Task B 网络业务（优先级 12 / 栈 6KB）。心跳属网络业务，与组队/收发共用 Task B 即可，避免新增 task 的栈开销。心跳 tick 集成在 Task B 的 NetworkService recv 分发循环中：每次循环迭代检查 `now - last_heartbeat >= period`，到期则发送 HEARTBEAT 包并更新 `last_heartbeat`。可变周期通过 `AtomicU8` 状态字 + `Instant` 比较（或 `esp_idf_svc::timer::EspTaskTimer` 周期回调）实现，状态切换时更新状态字即在下一次迭代生效。SHALL NOT 使用 `std::thread::spawn`（D12/D24 任务模型合规）。

### D4：状态相关周期用一个 `heartbeat_period(state) -> Duration` 函数集中
集中映射（技术设计 §13.1）：
- `Grouped(Idle)` 或 `Grouped(ChannelBusy)` → 5s
- `Hosting(_)` / `Joining(_)` → 1s
- `Grouped(Talking)` / `Grouped(Listening)` → 10s
- 熄屏待机（由 change 15 通过 `set_screen_off_heartbeat(true)` 标记）→ 10s

熄屏标记位独立于 VoiceState，因为熄屏时 VoiceState 可能仍是 Idle 但希望降频到 10s。change 15 设置该位即可生效，无需改本变更代码。

**D45 跨设备交叉引用**：change 10 的 `BootPress { screen_was_off: true }` 在熄屏时触发 PTT 而不点亮屏幕；change 15 的 ScreenPolicy 将 `BootGpioPress` 转发给 InputService 但不调用 `screen_on()`。两者互补而非冲突——熄屏时 BOOT 键既发起 PTT（change 10）又维持熄屏待机心跳 10s 周期（本变更 D4），共同支撑 PRD §15 熄屏待机低功耗 PTT 场景。

### D5：离线判定 15s，仅标离线不删成员
PRD §15.1 / §22.3 / 技术设计 §13.2：15s 未收到心跳/消息 → 标离线；不自动删除成员表条目。原因：成员可能临时离线（电池耗尽/超出范围），过段时间回来应能无缝恢复。正式移除需整组重建（§6.5 安全边界）。离线期间该成员不发不收，仍保留在 peer 表中。

### D6：恢复在线 = 收到任意消息/心跳即标在线
PRD §15.1 / 技术设计 §13.2：重新收到消息/心跳 → 在线。实现：ESP-NOW 收回调里无论包类型（HEARTBEAT/VOICE/TALK_STATE/CTRL），只要 sender 是组内 peer，即更新该 peer `last_seen = now`、`online = true`、`rssi_smoothed`。这样语音活动期间也能视为在线（与 §13.1 "稳定语音收发降频"协同：语音期间心跳降到 10s，但语音包本身更新 last_seen，不会误判离线）。

### D7：RSSI 4 格 = 指数平滑 + 滞回阈值
PRD §15.2 / 技术设计 §13.2：4 格粗颗粒，平滑 + 滞回避免频繁跳变；不显示精确值。算法：
1. 每次收包更新 `rssi_ewma = α·rssi_raw + (1-α)·rssi_ewma`（α=0.3，EWMA）
2. 映射到 4 格：`≥-55→4, ≥-65→3, ≥-75→2, ≥-85→1, else→0`
3. 滞回：当前格为 N 时，仅当 ewma 跨过 N±1 档阈值 ±3dB 才切换，避免在单档边界抖动

备选：中值滤波 / Kalman——EWM A 最简、内存一变量，RISC-V 单核足够。

### D8：去中心化 Peers 拓扑，无中心协调者
PRD §15.1 / §22.3：每台设备独立维护其他成员在线/信号状态。无 host 角色，收到对端心跳即本地更新该节点。原因：组内为全连接 mesh（§6.2 4 节点全网状 LMK），任意两台直接通信，无需中心转发状态。简化实现且无单点故障。

### D9：restore 成功后启动心跳，`leave_group` / 损坏清理时停止心跳
生命周期绑定：`restore_from_nvs()` 成功（进入 `Grouped(Idle)`）→ 启动心跳（置 run flag=true，`last_heartbeat=now`）；`leave_group()` 或 restore 失败回 `Idle` → 停止心跳（置 run flag=false）。Task B 下次迭代检查 run flag 即生效。

## Risks / Trade-offs

- **[EWMA α=0.3 在快速移动场景滞后]** → 用户快速远离时 RSSI 下降偏慢；可接受——PRD 明确仅 4 格粗颗粒且优先避免跳变，滞后优于抖动
- **[15s 离线阈值在弱网下误判]** → 弱网下心跳可能丢失；缓解：语音包/任意消息也更新 last_seen（D6），且 §13.1 稳定语音期降频到 10s 但语音活动本身维持在线判定
- **[冷启动 LMK 重算耗时]** → Curve25519 ECDH 在 ESP32-C6 160MHz 单核约 4 节点 × ~5ms = 20ms，远低于"瞬时"门槛（用户感知 <100ms）；可接受
- **[schema_ver 升级后旧组信息被清理]** → 用户需重新组队；符合 PRD §5.4 设计，非缺陷
- **[心跳 tick 与收发共用 Task B 的优先级反转]** → 心跳发送是非阻塞 `send_unicast`，不持有锁等待对端；组队/语音回调优先级由 ESP-NOW 收回调本身保证，心跳 tick 不会阻塞它们
- **[熄屏心跳 10s 与离线 15s 阈值接近]** → 熄屏待机时若两台设备心跳都 10s，丢一两个包就到 15s 阈值边缘；缓解：15s 阈值留有 5s 余量（50%），实际丢包率不支撑连续丢 1.5 个周期

## Migration Plan

无既有运行时需迁移。本变更在 change 09 已交付的 `IntercomService` 实现上增量：
1. 拉取本变更 commit
2. `cargo build` 确认编译通过
3. 烧录两台设备，先执行一次三阶段组队（change 09），重启其中一台
4. 验证：重启设备上电后无需重新组队即进入 Grouped(Idle)；对端看到其在线 + 4 格信号；断电一台 15s 后对端显示其离线但仍在成员表

回滚：`git revert <commit>`，回到 change 09 状态（冷启动回 Idle 需手动重新组队，无心跳）。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
