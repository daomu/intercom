## ADDED Requirements

### Requirement: 冷启动 NVS 恢复
冷启动时 `IntercomService::restore_from_nvs()` SHALL 调用 `StorageService::load_group()`；若返回 `Some(g)` 且 `g.schema_ver` 兼容当前固件，SHALL 依次执行：`NetworkService::init(g.channel)`（直接到工作信道，不经发现信道 1）→ 对 `g.peers` 中每个 peer 调用 `CryptoService::derive_lmk(g.my_priv_key, peer.pub_key)` 得到 LMK → `NetworkService::add_peer(peer.mac, lmk)` → 将状态置为 `Grouped(Idle)`。全程 SHALL NOT 产生任何 RF 收发交互。

#### Scenario: 组信息有效时瞬时恢复
- **WHEN** 设备上电且 NVS 中存在 schema_ver 兼容的有效组信息
- **THEN** `restore_from_nvs()` 返回 `Ok`，设备进入 `Grouped(Idle)`，全程无任何 ESP-NOW 发送/接收调用，恢复完成时延小于 100ms

#### Scenario: 直接初始化到工作信道
- **WHEN** 恢复流程调用 `NetworkService::init`
- **THEN** 传入的 channel 为 `g.channel`（持久化的工作信道），不经过发现信道 1

#### Scenario: LMK 加密表完整重建
- **WHEN** 恢复流程完成
- **THEN** 组内每个 peer 的 (mac, lmk) 均已通过 `add_peer` 写入 ESP-NOW 硬件加密表，后续收发走 AES-CCM 链路层加密

### Requirement: 组信息损坏或不兼容时清理回退
`load_group()` 返回 `None`、读取失败、或 `schema_ver` 不兼容当前固件时，`restore_from_nvs()` SHALL 调用 `StorageService::clear_group()` 清理组信息，状态保持 `Idle`（未组网），SHALL NOT 阻塞启动流程，SHALL NOT 回退默认组配置。

#### Scenario: 组信息损坏
- **WHEN** NVS 组信息损坏或字段非法
- **THEN** `clear_group()` 被调用，设备进入未组网 `Idle` 状态，启动继续不卡死

#### Scenario: schema_ver 不兼容
- **WHEN** NVS 中 `schema_ver` 大于固件当前版本或小于且不兼容
- **THEN** `clear_group()` 被调用，组信息清理，设备进入未组网状态

#### Scenario: 无组信息
- **WHEN** `load_group()` 返回 `None`
- **THEN** 不调用 `clear_group`（无内容可清），状态为 `Idle`，启动正常继续

### Requirement: 恢复成功后启动心跳任务
`restore_from_nvs()` 成功进入 `Grouped(Idle)` 后 SHALL 启动心跳任务；`leave_group()` 或恢复失败回 `Idle` 时 SHALL 停止心跳任务。

#### Scenario: 恢复成功启动心跳
- **WHEN** `restore_from_nvs()` 返回 `Ok` 且状态为 `Grouped(Idle)`
- **THEN** 心跳任务被启动，开始按 §13.1 周期发送 HEARTBEAT 包

#### Scenario: 退出组停止心跳
- **WHEN** 用户调用 `leave_group()` 或恢复失败
- **THEN** 心跳任务被停止，不再发送 HEARTBEAT 包

### Requirement: 恢复路径无 RF 交互
`restore_from_nvs()` 执行期间 SHALL NOT 调用任何会触发 ESP-NOW 发送或接收的接口；所有 LMK 计算与加密表写入 SHALL 在本地内存 + 硬件寄存器层面完成。

#### Scenario: 恢复期间空中静默
- **WHEN** 恢复流程执行中
- **THEN** RF 无任何发送/接收活动，对端无法通过空中探测感知本机正在恢复
