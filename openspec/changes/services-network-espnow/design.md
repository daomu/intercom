## Context

change 02（hal-bsp-drivers）已交付 ESP32-C6 radio 底层初始化：Wi-Fi 协议栈启动、`esp_now_init`、ESP-NOW 版本协商、收发基础回调注册。但该层只提供"裸 ESP-NOW 可收发"的能力，不提供对讲业务所需的：

- peer 表管理（含 LMK 写入硬件 AES-CCM 加密表）
- 加密单播 vs 明文广播的语义区分（PRD §18.5 运行时全部加密单播；§18.4 阶段1/2 组队用明文广播）
- 信道评估与切换（技术设计 §12.1 候选集 [1,6,11,13] RSSI 采样）
- 接收回调的非阻塞分发（回调内禁止阻塞，投递到队列由 Task B 处理）

技术设计 §3.3 已固化 `NetworkService` trait 签名与 `RecvEvent` / `NetError` 定义。PRD §18.5 明确组网后全部加密单播，应用层不重加密。PRD §19.1/§19.2 固化无线资源优先级 P0-P3，发言/接收期间绝对禁止切入其他无线任务。

三阶段组队（change 09）、PTT 语音（change 10）、心跳与冷启动恢复（change 12）全部依赖本期落地。

## Goals / Non-Goals

**Goals:**
- `NetworkService` trait + ESP-NOW 实现就绪，可被 IntercomService 直接 `Box<dyn NetworkService>` 注入
- `add_peer(mac, lmk)` 正确调用 `esp_now_add_peer_with_lmk` 将 16B LMK 写入硬件加密表（技术设计 §6.3 时序）
- `send_unicast` 走硬件 AES-CCM 加密；`send_broadcast` 走明文，且仅允许在 `DISCOVERY_CHANNEL=1` 调用
- `on_recv` 回调非阻塞——回调内仅构造 `RecvEvent` 并投递到队列，不调用任何可能阻塞的 API
- `evaluate_channels([1,6,11,13])` 返回推荐工作信道，采样逻辑快速（< 100ms），用于 host_confirm 阶段2（技术设计 §19.1）
- `RecvEvent.payload` 已由 ESP-NOW 硬件解密，上层无需再处理解密
- 无线资源优先级规则在文档与代码注释中固化，`evaluate_channels` / `set_channel` 在 P0 状态返回 `NetError::Channel` 拒绝执行

**Non-Goals:**
- LMK 的 ECDH 派生逻辑（由 change 03 CryptoService 提供，本期只接收 16B LMK）
- 对讲包格式封装（type/seq/payload 由 change 08 IntercomPacketFormat 定义，NetworkService 只透明传输 `&[u8]`）
- 三阶段组队状态机（change 09）
- PTT 语音循环与 jitter buffer（change 10/11）
- 心跳超时判定与冷启动恢复流程（change 12）
- Wi-Fi STA/AP 并发或 OTA 无线传输（PRD §19.2 已组网时默认禁止）
- 在线信道切换（技术设计 §12.3 明确不支持，切信道仅在组队阶段3统一切换）

## Decisions

### D1：实现位置 = `src/services/network/`，单一 ESP-NOW 实现
trait 放 `mod.rs`，ESP-NOW 实现放 `esp_now_impl.rs`，`RecvEvent`/`NetError` 放 `types.rs`。备选：多后端 trait（mock + 实机）——PRD 明确无 host simulator（change 01 D 决策），无需多后端，排除。

### D2：`add_peer` 直接透传 LMK 给 `esp_now_add_peer_with_lmk`
NetworkService 不持有密钥材料，不缓存 LMK。调用方（IntercomService/CryptoService）负责派生 LMK 后传入。理由：密钥材料集中管理减少泄漏面；硬件加密表是 ESP-NOW peer 表的唯一真实来源。备选：NetworkService 内部缓存 LMK 副本——增加状态同步复杂度，与硬件表可能不一致，排除。

### D3：`send_broadcast` 在信道 1 之外返回 `NetError::Channel`
PRD §18.4 阶段1/2 组队全部在 `DISCOVERY_CHANNEL=1` 明文广播；阶段3 切到目标信道后全部加密单播。`send_broadcast` 在非 1 信道调用是编程错误，本期返回错误而非 panic。备选：debug_assert + 静默发送——发行版仍会误发，违反加密边界，排除。

### D4：`on_recv` 回调 = `Box<dyn Fn(RecvEvent) + Send + Sync>`，实现内 spawn 投递
ESP-NOW 收包回调在 esp-idf-svc 内部 task 触发。实现将原始包构造为 `RecvEvent`（payload 已硬件解密，拷入 `heapless::Vec<u8, 250>`），通过 `std::sync::Mutex<Option<Box<dyn Fn>>>` 调用用户回调。回调存储使用 `Option` 包裹（D39）：调用 `on_recv` 时替换 `Option` 内的 Box，旧回调被 drop 释放，允许上层在不同阶段复用同一 service 实例并替换回调。用户回调 SHALL 仅做 enqueue，不阻塞。备选：直接在回调内处理业务——阻塞 ESP-NOW 收包 task 会导致丢包，排除。

### D5：`evaluate_channels` 实现 = 切信道 + `esp_wifi_sta_get_rssi` 采样 + 回原信道
对每个候选信道：`set_channel(ch)` → 等待 ~20ms 稳定 → 读 `get_rssi()` → 选 RSSI 最高（干扰最低）的信道，以 `Ok(ch)` 返回。总耗时 < 100ms。仅在 host_confirm 阶段2（未进入 P0）调用；radio_priority == 0 时直接返回 `Err(NetError::Channel)`（D13 guard）。备选：被动监听 ESP-NOW 包间空闲度——采样时间长且不可控，排除。

### D6：无线优先级规则 = 代码级 guard + 文档固化
NetworkService 持有一个 `AtomicU8` 表示当前 radio_priority（0=P0 .. 3=P3），由 IntercomService 在状态转换时设置。`evaluate_channels` / `set_channel` 在 priority == 0（P0 发言/接收）时返回 `Err(NetError::Channel)` 拒绝执行。备选：纯文档约定无代码 guard——PRD §19.2「绝对禁止」要求强保证，排除。

`set_radio_priority` 是为 PRD §19.2 radio resource management 配套添加的 D-guard 方法，不属于技术设计 §3.3 的对讲业务方法集（§3.3 列出 10 个方法）。NetworkService trait extends §3.3's 10 methods with `set_radio_priority`——This is a deliberate addition, not a deviation（D23）。`evaluate_channels` 的返回类型由 §3.3 的 `u8` 调整为 `Result<u8, NetError>` 以承载 guard 拒绝路径（D13）。

### D7：`RecvEvent.rssi` = 接收时硬件上报值
ESP-NOW 收包回调携带 `rx_ctrl->rssi`，直接填入 `RecvEvent.rssi: i8`。`get_rssi()` 返回当前信道最近一次采样值。备选：滑动平均——增加复杂度，上层 IntercomService 心跳模块自己做平滑（技术设计 §13.2「RSSI 平滑+滞回」），本期只提供原始值，排除。

### D8：`clear_peers` = 遍历 `esp_now_peer_list` 逐个 remove
esp-idf-svc 不提供批量清空 API。实现内部维护已注册 peer 的 MAC 列表（`Mutex<Vec<[u8;6]>>`），`clear_peers` 遍历调用 `esp_now_del_peer`。备选：仅清本地列表不调硬件——硬件 peer 表残留导致旧 LMK 仍可解密，安全边界破坏，排除。

### D9：`init` 不重复 `esp_now_init`，仅设置工作信道与清空 peer 表
change 02 已完成 `esp_now_init`。本期 `init(channel)` 仅：设置内部 current_channel、调用 `clear_peers()`、配置 ESP-NOW primary/second channel。备选：重复 init——esp-idf-svc `EspNow` 是单例，二次 init 返回错误，排除。

## Risks / Trade-offs

- **[ESP-NOW peer 表容量上限]** → ESP32-C6 硬件 peer 表上限 20（加密），4 节点全网状每节点 3 个 peer，远低于上限；若未来扩容需测试
- **[evaluate_channels 切信道期间丢包]** → 仅在阶段2（未进入 P0 语音）调用；采样总耗时 < 100ms，阶段2 此期间 Host 不发 PAIR_BEACON，可接受短时静默
- **[on_recv 回调内 panic 传播到 ESP-NOW task]** → 用户回调 SHALL 不 panic；实现用 `catch_unwind` 包裹（若 `Fn` panic 则 log + 丢弃此包，不传播）
- **[esp-idf-svc esp_now API 版本漂移]** → change 01 锁定 `esp-idf-svc = "0.49"`；本期实现按 0.49 API 编写，升级时需回归
- **[send_unicast 对未注册 peer 的 MAC 发送]** → 硬件无 LMK 时 ESP-NOW 会以明文发送；本期 `send_unicast` 前检查内部 peer 列表，未注册返回 `NetError::TxFail`，防止误发明文
- **[radio_priority AtomicU8 与 IntercomService 状态不同步]** → 依赖 change 09+ 在每次状态转换时调用 `set_radio_priority`；本期提供 setter，不保证调用方正确使用
- **[明文广播 payload 限制 ≤ 250B]** → ESP-NOW 单包上限 250B；组队包 DIRECTORY_BROADCAST 在 4 节点时 = 15 + 4×38 = 167B，未超限

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 拉取本变更 commit
2. 确认 change 02 的 `EspNow` 单例可被 `network/esp_now_impl.rs` 引用
3. `cargo build` 验证编译
4. 烧录两台设备，在阶段1 验证明文广播互收（需 change 09 提供测试入口，本期仅单元验证 trait 方法可调用）
5. 后续 change 09 落地后做端到端组队验证

回滚：`git revert <commit>`，移除 `src/services/network/` 目录与 `services/mod.rs` 中的注册行。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
