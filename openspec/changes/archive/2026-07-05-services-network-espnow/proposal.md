## Why

17 个变更提案的第 6 个。change 02（hal-bsp-drivers）已交付 ESP-NOW radio 底层初始化（Wi-Fi 协议栈启动、`esp_now_init`、ESP-NOW 版本协商），但尚无上层 Service 封装对讲业务所需的 peer 管理 / 加密收发 / 信道评估能力。对讲三阶段组队、PTT 语音收发、心跳在线、冷启动恢复（change 09/10/11/12）全部依赖一个可信的 `NetworkService` 抽象，本期将其落地为可被 IntercomService 直接调用的 trait + 实现。

## What Changes

- 新增 `src/services/network/mod.rs`：定义 `NetworkService` trait（技术设计 §3.3 全部方法签名：`init` / `set_channel` / `add_peer` / `remove_peer` / `clear_peers` / `send_unicast` / `send_broadcast` / `on_recv` / `evaluate_channels` / `get_rssi`）
- 新增 `src/services/network/esp_now_impl.rs`：基于 `esp-idf-svc::esp_now` 的具体实现，封装以下硬件交互：
  - `init(channel)`：在已 `esp_now_init` 的 radio 上配置工作信道（不重复协议栈启动，依赖 change 02）
  - `add_peer(mac, lmk)`：调用 `esp_now_add_peer_with_lmk` 将 16B LMK 写入 ESP-NOW 硬件 AES-CCM 加密表（技术设计 §6.3 时序）
  - `send_unicast(dst, payload)`：加密单播发送（payload 由 Intercom 层封装，硬件自动 AES-CCM 加密/解密）
  - `send_broadcast(payload)`：明文广播发送，仅允许在信道 1（`DISCOVERY_CHANNEL`）调用，PRD §18.4 阶段1/2 组队使用
  - `on_recv(cb)`：注册接收回调，回调内禁止阻塞，必须将 `RecvEvent` 投递到队列由 Task B 处理
  - `evaluate_channels(candidates)`：在候选集 [1,6,11,13] 上做 RSSI/空闲度快速采样，返回推荐工作信道（技术设计 §12.1）
  - `get_rssi()`：返回当前信道 RSSI
- 新增 `RecvEvent` 结构（`src_mac: [u8;6]` / `payload: heapless::Vec<u8, 250>` / `rssi: i8`），payload 已由 ESP-NOW 硬件解密（D18：使用 `heapless::Vec` 避免 recv 路径堆分配，ESP-NOW 单包上限 ~250B）
- 新增 `NetError` 枚举（`Init` / `Channel` / `PeerFull` / `TxFail` / `Rx`）
- 文档级固化无线资源管理优先级规则（PRD §19.1）：P0 发言/接收 > P1 已组网待机 > P2 组队搜索 > P3 其他无线；NetworkService 不直接调度优先级，但 SHALL 在 P0 状态拒绝 `evaluate_channels` 等会切信道或扫描的调用以避免中断实时语音

## Capabilities

### New Capabilities
- `network-service`: ESP-NOW 上层网络服务抽象——peer 管理（含 LMK 硬件加密表写入）、加密单播 / 明文广播收发、接收回调非阻塞分发、候选信道 RSSI 评估、当前信道 RSSI 查询

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/services/network/{mod.rs, esp_now_impl.rs, types.rs}`；在 `src/services/mod.rs` 注册 `pub mod network;`
- **依赖**：依赖 change 02 提供的 ESP-NOW radio 初始化（`esp_now_init` 已完成）；依赖 `esp-idf-svc` 的 `esp_now` 模块（已在 change 01 声明）；新增 `heapless` crate 依赖（用于 `RecvEvent.payload: heapless::Vec<u8, 250>`，D18），在 `Cargo.toml` 添加 `heapless = "0.8"`
- **后续变更**：
  - change 09（三阶段组队）调用 `init` / `add_peer` / `clear_peers` / `send_broadcast` / `evaluate_channels` / `set_channel`
  - change 10（PTT 语音）调用 `send_unicast` / `on_recv`
  - change 12（心跳/恢复）调用 `on_recv` / `send_unicast` / 冷启动 `init` + `add_peer` 直接到工作信道
- **无线电约束**：ESP32-C6 单 RF，ESP-NOW 与 Wi-Fi 半双工共享；NetworkService SHALL NOT 在 P0（发言/接收）期间执行信道扫描或 `evaluate_channels`，以遵守 PRD §19.2「发言/接收绝对禁止切入其他无线任务」
- **安全边界**：NetworkService 只负责传输；payload 加密由 ESP-NOW 硬件 AES-CCM 完成，应用层不再重加密（PRD §18.5）；LMK 派生由 change 03 的 CryptoService 完成，本期仅接收 16B LMK 并写入硬件
