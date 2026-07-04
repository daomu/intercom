## 1. 模块骨架与类型定义

- [ ] 1.1 新增 `src/services/network/mod.rs`：定义 `pub trait NetworkService: Send + Sync`，列出 11 个方法签名（init/set_channel/add_peer/remove_peer/clear_peers/send_unicast/send_broadcast/on_recv/evaluate_channels/get_rssi/set_radio_priority）。其中前 10 个与 §3.3 一致，`set_radio_priority` 为 PRD §19.2 radio guard 配套的 deliberate addition（D23）；`evaluate_channels` 返回 `Result<u8, NetError>` 而非裸 `u8`（D13）
- [ ] 1.2 新增 `src/services/network/types.rs`：定义 `pub struct RecvEvent { pub src_mac: [u8;6], pub payload: heapless::Vec<u8, 250>, pub rssi: i8 }`（D18：payload 使用 no-alloc 容器，ESP-NOW 单包上限 ~250B）与 `pub enum NetError { Init, Channel, PeerFull, TxFail, Rx }`，并实现 `std::fmt::Debug` 与 `std::error::Error` for `NetError`
- [ ] 1.3 在 `src/services/network/mod.rs` 中 `pub mod types; pub use types::{RecvEvent, NetError};`
- [ ] 1.4 在 `src/services/mod.rs` 追加 `pub mod network;`

## 2. ESP-NOW 实现结构

- [ ] 2.1 新增 `src/services/network/esp_now_impl.rs`：定义 `pub struct EspNowNetworkService`，持有 `EspNow` 单例引用（来自 change 02）、`Mutex<Vec<[u8;6]>>` peer 列表、`Mutex<u8>` current_channel、`AtomicU8` radio_priority、`Mutex<Option<Box<dyn Fn(RecvEvent) + Send + Sync>>>` recv 回调
- [ ] 2.2 实现 `new(esp_now: EspNow) -> Self`，初始化各字段，current_channel 初始为 `BoardProfile::DISCOVERY_CHANNEL`
- [ ] 2.3 在 `mod.rs` 追加 `pub mod esp_now_impl; pub use esp_now_impl::EspNowNetworkService;`

## 3. init / set_channel 实现

- [ ] 3.1 实现 `init(&self, channel)`：设置 current_channel = channel，调用 `clear_peers()`，配置 ESP-NOW primary channel，返回 `Ok(())`；不调用 `esp_now_init`（由 change 02 完成）
- [ ] 3.2 实现 `set_channel(&self, ch)`：检查 radio_priority，若 == 0 返回 `Err(NetError::Channel)`；否则设置 ESP-NOW channel 与 current_channel，返回 `Ok(())`
- [ ] 3.3 实现 `set_radio_priority(&self, p)`：`self.radio_priority.store(p, Ordering::SeqCst)`

## 4. peer 管理实现

- [ ] 4.1 实现 `add_peer(&self, mac, lmk)`：调用 `esp_now_add_peer_with_lmk`（或等价 esp-idf-svc API），成功后将 mac 加入内部 peer 列表，失败映射为 `NetError::PeerFull`
- [ ] 4.2 实现 `remove_peer(&self, mac)`：调用 `esp_now_del_peer`，从内部列表移除
- [ ] 4.3 实现 `clear_peers(&self)`：遍历内部 peer 列表逐个 `esp_now_del_peer`，清空列表，返回 `Ok(())`

## 5. 发送实现

- [ ] 5.1 实现 `send_unicast(&self, dst, payload)`：检查 dst 在内部 peer 列表（未注册返回 `Err(NetError::TxFail)`）；调用 `esp_now_send(dst, payload)`，失败返回 `Err(NetError::TxFail)`
- [ ] 5.2 实现 `send_broadcast(&self, payload)`：检查 current_channel == 1（否则返回 `Err(NetError::Channel)`）；调用 `esp_now_send(BROADCAST_MAC, payload)` 明文发送

## 6. 接收回调实现

- [ ] 6.1 实现 `on_recv(&self, cb)`：将 `Box<dyn Fn(RecvEvent) + Send + Sync>` 存入 `Mutex<Option<Box<dyn Fn(RecvEvent) + Send + Sync>>>`（D39：替换式存储，旧回调 drop 释放）
- [ ] 6.2 在 ESP-NOW 收包回调（由 change 02 注册的基础回调）中：构造 `RecvEvent { src_mac, payload: heapless::Vec::from_slice(data).unwrap_or_default(), rssi }`（payload 已硬件解密，拷入 no-alloc 容器，D18），用 `catch_unwind` 调用用户回调；panic 时 `error!` 并丢弃
- [ ] 6.3 验证回调路径不持有任何 Mutex 超过构造 RecvEvent 的时间（< 1ms）；验证二次 `on_recv` 调用后旧回调被 drop、新回调生效（D39 回调替换场景）

## 7. 信道评估与 RSSI

- [ ] 7.1 实现 `get_rssi(&self)`：返回 `esp_wifi_sta_get_rssi` 或 ESP-NOW 收包最近上报的 RSSI（`i8`）
- [ ] 7.2 实现 `evaluate_channels(&self, candidates) -> Result<u8, NetError>`：检查 radio_priority == 0 则返回 `Err(NetError::Channel)`（D13）；记录原 current_channel，对每个候选 ch 执行 set_channel(ch) → sleep 20ms → get_rssi() → 记录；恢复原 current_channel；返回 `Ok(RSSI 最高的 ch)`；空候选集返回 `Ok(原 channel)`

## 8. 构建验证

- [ ] 8.1 执行 `cargo build`，确认 `src/services/network/` 全部编译通过，无错误
- [ ] 8.2 执行 `cargo clippy`，确认无 warning（除第三方 crate）

## 9. 单元验证（on-device）

- [ ] 9.1 烧录两台设备，在 `main.rs` 临时构造 `EspNowNetworkService`，调用 `init(1)` + `add_peer` + `send_broadcast`，验证对端 `on_recv` 收到 `RecvEvent`（payload 一致、rssi 合理）
- [ ] 9.2 验证 `send_unicast` 加密路径：注册 LMK 后单播，对端收到解密后 payload；未注册 MAC 返回 `Err(NetError::TxFail)`
- [ ] 9.3 验证 `send_broadcast` 在信道 6 返回 `Err(NetError::Channel)`，在信道 1 成功
- [ ] 9.4 验证 `evaluate_channels(&[1,6,11,13])` 返回 `Ok(1..13 区间值)` 且耗时 < 100ms（D13）
- [ ] 9.5 验证 `set_radio_priority(0)` 后 `evaluate_channels` 与 `set_channel` 返回 `Err(NetError::Channel)`（D13）
- [ ] 9.6 移除 `main.rs` 临时验证代码，保留模块注册

## 10. 收尾

- [ ] 10.1 提交 commit：`feat: add NetworkService trait + ESP-NOW impl (change 06/17)`
- [ ] 10.2 在 commit message 注明后续 change 09/10/12 将消费此 trait，引用技术设计 §3.3 与本变更 design.md
