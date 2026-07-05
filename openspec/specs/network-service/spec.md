## ADDED Requirements

### Requirement: NetworkService trait 定义
项目 SHALL 在 `src/services/network/mod.rs` 定义 `NetworkService` trait，包含以下方法签名（与技术设计 §3.3 一致）：
- `fn init(&self, channel: u8) -> Result<(), NetError>`
- `fn set_channel(&self, ch: u8) -> Result<(), NetError>`
- `fn add_peer(&self, mac: &[u8; 6], lmk: &[u8; 16]) -> Result<(), NetError>`
- `fn remove_peer(&self, mac: &[u8; 6]) -> Result<(), NetError>`
- `fn clear_peers(&self) -> Result<(), NetError>`
- `fn send_unicast(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError>`
- `fn send_broadcast(&self, payload: &[u8]) -> Result<(), NetError>`
- `fn on_recv(&self, cb: Box<dyn Fn(RecvEvent) + Send + Sync>)`
- `fn evaluate_channels(&self, candidates: &[u8]) -> Result<u8, NetError>`
- `fn get_rssi(&self) -> i8`
- `fn set_radio_priority(&self, p: u8)`（P0=0..P3=3，用于 guard 信道扫描）

`evaluate_channels` 返回 `Result<u8, NetError>` 而非裸 `u8`，以便在 P0 等禁止扫描状态下返回 `Err(NetError::Channel)`（见 D13）。

trait SHALL 标注 `Send + Sync` 以便跨 task 注入。

#### Scenario: trait 可被 Box 注入
- **WHEN** IntercomService 构造时接收 `Box<dyn NetworkService>`
- **THEN** 该 trait object 可在 Task B（网络业务 task，优先级 12）中调用任意方法，编译期通过 Send + Sync 约束

#### Scenario: trait 方法数与技术设计 §3.3 的关系
- **WHEN** 检查 `src/services/network/mod.rs` 的 trait 定义
- **THEN** NetworkService trait extends 技术设计 §3.3 列出的 10 个方法（init/set_channel/add_peer/remove_peer/clear_peers/send_unicast/send_broadcast/on_recv/evaluate_channels/get_rssi）之外，增加 `set_radio_priority`（D-guard for PRD §19.2 radio resource management）。This is a deliberate addition, not a deviation——`set_radio_priority` 不属于 §3.3 的对讲业务方法集，而是为 PRD §19.2 无线资源优先级 guard 服务的配套方法。`evaluate_channels` 的返回类型由 §3.3 的 `u8` 调整为 `Result<u8, NetError>` 以承载 guard 拒绝（D13）。

### Requirement: ESP-NOW 实现
项目 SHALL 在 `src/services/network/esp_now_impl.rs` 提供 `EspNowNetworkService` 结构实现 `NetworkService` trait，基于 `esp-idf-svc::esp_now::EspNow`（由 change 02 初始化的单例）。

#### Scenario: init 设置工作信道并清空 peer 表
- **WHEN** 调用 `network.init(6)`
- **THEN** 内部 current_channel 设为 6，已注册 peer 列表被清空，ESP-NOW primary channel 配置为 6，返回 `Ok(())`

#### Scenario: init 不重复调用 esp_now_init
- **WHEN** 多次调用 `network.init(channel)`
- **THEN** 不触发 `esp_now_init` 二次调用（由 change 02 已完成），仅更新信道与清空 peer，返回 `Ok(())`

### Requirement: add_peer 写入 LMK 到硬件 AES-CCM 表
`add_peer(mac, lmk)` SHALL 调用 `esp_now_add_peer_with_lmk`（或等价 esp-idf-svc API）将 16 字节 LMK 写入 ESP-NOW 硬件加密表，使后续 `send_unicast` 到该 MAC 的包由硬件 AES-CCM 加密、对端硬件解密。NetworkService SHALL NOT 缓存或派生 LMK，仅透传。

#### Scenario: 注册 peer 后单播自动加密
- **WHEN** `network.add_peer(&peer_mac, &lmk)` 成功后调用 `network.send_unicast(&peer_mac, &payload)`
- **THEN** 硬件使用注册的 LMK 对 payload 做 AES-CCM 加密后发送，对端（已注册本机 MAC+同一 LMK）收到后硬件自动解密，`on_recv` 回调收到 `RecvEvent.payload` 为明文

#### Scenario: 4 节点全网状注册
- **WHEN** 本机依次对 3 个对端调用 `add_peer`
- **THEN** 硬件 peer 表含 3 条加密 peer 条目，每条独立 LMK，符合技术设计 §6.2 全网状 LMK 表

#### Scenario: peer 表满返回错误
- **WHEN** 硬件 peer 表已满（超过 ESP-NOW 加密 peer 上限）时调用 `add_peer`
- **THEN** 返回 `Err(NetError::PeerFull)`，不 panic

### Requirement: send_unicast 拒绝未注册 peer
`send_unicast(dst, payload)` SHALL 在发送前检查 `dst` 是否已在内部 peer 列表中注册；未注册时返回 `Err(NetError::TxFail)`，防止误发明文包（ESP-NOW 无 LMK 时会明文发送）。

#### Scenario: 未注册 MAC 单播被拒
- **WHEN** 调用 `network.send_unicast(&unregistered_mac, &payload)`
- **THEN** 返回 `Err(NetError::TxFail)`，不调用 `esp_now_send`

#### Scenario: 已注册 MAC 单播成功
- **WHEN** 调用 `network.send_unicast(&registered_mac, &payload)` 且硬件发送成功
- **THEN** 返回 `Ok(())`，payload 由硬件 AES-CCM 加密发送

### Requirement: send_broadcast 仅在信道 1 允许
`send_broadcast(payload)` SHALL 仅在 current_channel == 1（`DISCOVERY_CHANNEL`）时发送明文广播；在其他信道调用 SHALL 返回 `Err(NetError::Channel)`。广播使用 ESP-NOW 广播 MAC（`ff:ff:ff:ff:ff:ff`），不经过 LMK 加密。

#### Scenario: 信道 1 广播成功
- **WHEN** current_channel == 1 且调用 `network.send_broadcast(&payload)`
- **THEN** 通过 ESP-NOW 广播 MAC 明文发送，返回 `Ok(())`

#### Scenario: 非 1 信道广播被拒
- **WHEN** current_channel == 6 且调用 `network.send_broadcast(&payload)`
- **THEN** 返回 `Err(NetError::Channel)`，不发送，防止在加密工作信道误发明文

### Requirement: RecvEvent 结构
项目 SHALL 定义 `RecvEvent` 结构（`src/services/network/types.rs`）：`pub struct RecvEvent { pub src_mac: [u8; 6], pub payload: heapless::Vec<u8, 250>, pub rssi: i8 }`。`payload` SHALL 为已由 ESP-NOW 硬件解密后的明文，上层无需再处理解密。`payload` 使用 `heapless::Vec<u8, 250>`（ESP-NOW 单包最大 payload ~250 字节）以避免接收路径上的堆分配（D18）；recv 回调在 ESP-NOW 收包 task 中触发，no-alloc 容器保证回调不阻塞分配器。

#### Scenario: 收到加密单播包
- **WHEN** 对端用注册 LMK 加密发送单播包，本机硬件解密后触发 on_recv 回调
- **THEN** `RecvEvent.src_mac` = 对端 MAC，`RecvEvent.payload` = 解密后明文，`RecvEvent.rssi` = 接收时硬件上报 RSSI

#### Scenario: 收到明文广播包
- **WHEN** 对端在信道 1 明文广播，本机收到后触发 on_recv 回调
- **THEN** `RecvEvent.payload` = 原始明文 payload，`RecvEvent.rssi` = 接收时 RSSI

### Requirement: on_recv 回调非阻塞
`on_recv(cb)` 注册的回调函数 SHALL 在 ESP-NOW 收包 task 中被调用；回调内禁止阻塞调用（如 `std::sync::Mutex` 长时间持有、`sleep`、网络重发）。实现 SHALL 用 `catch_unwind` 包裹回调，panic 时 log + 丢弃此包，不传播到 ESP-NOW task。实现 SHALL 在内部使用 `Mutex<Option<Box<dyn Fn(RecvEvent) + Send + Sync>>>` 存储回调（D39）：调用 `on_recv` 时替换 `Option` 内的回调 Box，旧回调（若存在）被 drop 释放；ESP-NOW 收包路径用 `Mutex::lock` 短时持有以读取回调引用再调用。此设计允许上层在不同阶段（组队/语音/心跳）复用同一 NetworkService 实例并替换回调，无需重建 service。

#### Scenario: 回调仅 enqueue 不阻塞
- **WHEN** ESP-NOW 收到一个包并触发回调
- **THEN** 回调函数执行时间 < 1ms（仅构造 `RecvEvent` 并投递到队列），不阻塞后续包接收

#### Scenario: 回调 panic 不影响 ESP-NOW task
- **WHEN** 用户回调内 panic
- **THEN** 实现的 `catch_unwind` 捕获 panic，记录 error 日志并丢弃此包，ESP-NOW 收包 task 继续运行

#### Scenario: 回调替换
- **WHEN** 先后调用 `network.on_recv(cb_a)` 与 `network.on_recv(cb_b)`，期间收到一个包
- **THEN** 第二次 `on_recv` 后 `cb_a` 被 drop，`cb_b` 被装入 `Mutex<Option<...>>`；此后 ESP-NOW 收包路径调用的是 `cb_b`，不再调用 `cb_a`。`cb_a` 的 drop 发生在 `on_recv` 调用线程而非 ESP-NOW 收包 task，不影响收包路径时序。

### Requirement: NetError 枚举
项目 SHALL 定义 `NetError` 枚举（`src/services/network/types.rs`）：`pub enum NetError { Init, Channel, PeerFull, TxFail, Rx }`，覆盖初始化失败、信道非法、peer 表满、发送失败、接收异常五类错误。

#### Scenario: 错误可被调用方 match
- **WHEN** 调用方 `match network.add_peer(...) { Err(NetError::PeerFull) => ..., _ => ... }`
- **THEN** 编译期通过，每个变体有明确语义

### Requirement: evaluate_channels 候选信道采样
`evaluate_channels(candidates) -> Result<u8, NetError>` SHALL 对每个候选信道执行：切信道 → 等待稳定（~20ms）→ 读 RSSI → 选 RSSI 最高（干扰最低）的信道以 `Ok(ch)` 返回。总耗时 SHALL < 100ms。候选集 SHALL 来自调用方传入（技术设计 §12.1 固定为 `[1,6,11,13]`），本方法不硬编码候选集。当 radio_priority == 0（P0 发言/接收）时 SHALL 返回 `Err(NetError::Channel)` 不执行采样（D13 guard）。

#### Scenario: host_confirm 阶段2 选信道
- **WHEN** Host 在阶段2 调用 `network.evaluate_channels(&[1,6,11,13])`
- **THEN** 返回 `Ok(target_channel)`（4 个信道中 RSSI 最高的一个），总采样耗时 < 100ms，采样结束后 current_channel 恢复为调用前的信道

#### Scenario: 候选集为空
- **WHEN** 调用 `network.evaluate_channels(&[])`
- **THEN** 返回 `Ok(current_channel)`（不切换），不 panic

### Requirement: get_rssi 返回当前信道 RSSI
`get_rssi()` SHALL 返回当前信道最近一次硬件上报的 RSSI 值（`i8`，单位 dBm）。

#### Scenario: 调用后返回有效值
- **WHEN** radio 已初始化且至少收发过一次后调用 `network.get_rssi()`
- **THEN** 返回 -128..0 区间的 dBm 值

### Requirement: 无线资源优先级 guard
项目 SHALL 在 `NetworkService` 提供 `set_radio_priority(p: u8)` 方法（0=P0 发言/接收, 1=P1 已组网待机, 2=P2 组队搜索, 3=P3 其他无线）。`evaluate_channels` 与 `set_channel` SHALL 在 priority == 0（P0）时返回 `Err(NetError::Channel)` 拒绝执行，遵守 PRD §19.2「发言/接收绝对禁止切入其他无线任务」。

#### Scenario: P0 状态拒绝信道扫描
- **WHEN** `set_radio_priority(0)` 后调用 `network.evaluate_channels(&[1,6,11,13])`
- **THEN** 返回 `Err(NetError::Channel)`，不执行信道切换采样

#### Scenario: P0 状态拒绝切信道
- **WHEN** `set_radio_priority(0)` 后调用 `network.set_channel(11)`
- **THEN** 返回 `Err(NetError::Channel)`，current_channel 不变

#### Scenario: P1 状态允许信道扫描
- **WHEN** `set_radio_priority(1)` 后调用 `network.evaluate_channels(&[1,6,11,13])`
- **THEN** 正常执行采样并返回推荐信道

### Requirement: clear_peers 清空硬件 peer 表
`clear_peers()` SHALL 遍历内部已注册 peer MAC 列表，逐个调用 `esp_now_del_peer` 删除硬件 peer 条目，并清空内部列表。确保旧 LMK 不残留可解密旧包。

#### Scenario: 切信道前清空 peer
- **WHEN** 阶段3 切信道前调用 `network.clear_peers()`
- **THEN** 硬件 peer 表所有加密条目被删除，后续 `add_peer` 重建全网状 LMK

#### Scenario: 空表清空不报错
- **WHEN** peer 列表为空时调用 `network.clear_peers()`
- **THEN** 返回 `Ok(())`，无副作用

### Requirement: 模块注册
`src/services/mod.rs` SHALL 包含 `pub mod network;`，使后续变更可通过 `crate::services::network::NetworkService` 引用。

#### Scenario: 后续变更引用 trait
- **WHEN** change 09 在 `src/intercom/` 中 `use crate::services::network::NetworkService`
- **THEN** 编译期解析成功，无需新建顶层目录
