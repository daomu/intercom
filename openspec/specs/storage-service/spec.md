## ADDED Requirements

### Requirement: StorageService trait 定义
项目 SHALL 定义 `StorageService` trait（`Send + Sync`），暴露以下方法签名：`load_settings(&self) -> Settings`、`save_settings(&self, s: &Settings) -> Result<(), StorageError>`、`reset_settings(&self) -> Result<(), StorageError>`、`load_group(&self) -> Option<GroupInfo>`、`save_group(&self, g: &GroupInfo) -> Result<(), StorageError>`、`clear_group(&self) -> Result<(), StorageError>`、`load_diag(&self) -> DiagInfo`、`inc_abnormal_boot(&self) -> Result<(), StorageError>`、`clear_diag(&self) -> Result<(), StorageError>`。

#### Scenario: trait 可被实现与引用
- **WHEN** 后续变更在 `src/services/storage/mod.rs` 引用 `StorageService` trait
- **THEN** 编译期解析到该 trait 定义，且任何实现该 trait 的类型可被多态调用

### Requirement: NvsStorage 实现
项目 SHALL 提供 `NvsStorage` 结构体实现 `StorageService` trait，使用 ESP-IDF NVS API，操作 3 个命名空间：`sys`（系统设置）、`group`（组信息）、`diag`（诊断信息）。`NvsStorage::new()` SHALL 返回 `Result<Self, StorageError>` 且不 panic。

#### Scenario: 实例化 NvsStorage
- **WHEN** 在已初始化 NVS flash 的环境下调用 `NvsStorage::new()`
- **THEN** 返回 `Ok(NvsStorage { ... })`，内部持有 3 个命名空间的句柄

#### Scenario: NVS flash 未初始化时优雅失败
- **WHEN** 在 NVS flash 未初始化的环境下调用 `NvsStorage::new()`
- **THEN** 返回 `Err(StorageError::Io)`，不 panic

### Requirement: sys 命名空间键布局
`sys` 命名空间 SHALL 包含以下键：`schema_ver`（u16）、`device_name`（str）、`volume`（u8）、`muted`（bool）、`brightness`（u8）、`screen_off_sec`（u32）。键名与类型 SHALL 与技术设计 §7.2 一致。

#### Scenario: 读取 sys 全部键
- **WHEN** 调用 `load_settings()` 且 NVS 中 sys 命名空间所有键存在且类型合法
- **THEN** 返回 `Settings` 结构体，字段值与 NVS 中存储的值一致

#### Scenario: sys 命名空间缺失字段
- **WHEN** 调用 `load_settings()` 且某个键缺失或类型不匹配
- **THEN** 返回的 `Settings` 中该字段回退到默认值（device_name=随机生成名、volume=50、muted=false、brightness=80、screen_off_sec=30），且 `schema_ver` 设为固件当前版本

### Requirement: group 命名空间键布局
`group` 命名空间 SHALL 包含以下键：`schema_ver`（u16）、`my_priv_key`（blob[32]）、`peers`（blob，PeerEntry 数组序列化，上限 4×38=152 字节）、`mode`（u8，0=Clear/1=Free）、`channel`（u8）、`last_state`（u8）。键名与类型 SHALL 与技术设计 §7.3 一致。

#### Scenario: 保存并加载 group
- **WHEN** 调用 `save_group(&g)` 后再调用 `load_group()`
- **THEN** 返回的 `GroupInfo` 与保存的 `g` 字段值一致（含 `my_priv_key`、`peers`、`mode`、`channel`、`last_state`）

#### Scenario: 无组信息时返回 None
- **WHEN** 调用 `load_group()` 且 group 命名空间为空或不存在
- **THEN** 返回 `None`，不返回错误

### Requirement: diag 命名空间键布局
`diag` 命名空间 SHALL 包含以下键：`abnormal_boot_cnt`（u32）、`safe_boot_flag`（bool）、`last_reset_reason`（u32）。键名与类型 SHALL 与技术设计 §7.4 一致。

#### Scenario: 加载 diag 默认值
- **WHEN** 调用 `load_diag()` 且 diag 命名空间为空
- **THEN** 返回 `DiagInfo { abnormal_boot_cnt: 0, safe_boot_flag: false, last_reset_reason: 0 }`

#### Scenario: 递增异常启动计数
- **WHEN** 当前 `abnormal_boot_cnt` 为 2 时调用 `inc_abnormal_boot()`
- **THEN** NVS 中 `abnormal_boot_cnt` 变为 3，后续 `load_diag()` 返回 3

> **Note (D29)**：`inc_abnormal_boot()` 方法在 `StorageService` trait 中始终存在，但调用方决定是否调用。仅当 `PowerService::reset_reason() != PowerOn`（即非正常上电复位）时才调用此方法。正常上电复位不递增异常启动计数。调用判定逻辑在 change 16（safety-diagnostics）实现。

### Requirement: schema_ver 兼容性规则
`NvsStorage` SHALL 实现技术设计 §7.5 定义的 schema_ver 兼容性规则：NVS schema_ver == 固件版本 → 正常加载；NVS schema_ver < 固件版本 → sys 回退默认值 + group 清理；NVS schema_ver > 固件版本 → group 清理；读取失败/字段非法 → group 清理。固件当前 schema_ver SHALL 为编译期常量。

#### Scenario: schema_ver 匹配时正常加载
- **WHEN** NVS 中 `schema_ver` == 固件常量 `SCHEMA_VER`
- **THEN** `load_settings()` 与 `load_group()` 返回 NVS 中存储的数据，无清理动作

#### Scenario: NVS schema_ver 低于固件版本
- **WHEN** NVS 中 sys `schema_ver` = 0 且固件 `SCHEMA_VER` = 1
- **THEN** `load_settings()` 返回默认值 Settings，`load_group()` 返回 None 且 group 命名空间被清理

#### Scenario: NVS schema_ver 高于固件版本
- **WHEN** NVS 中 group `schema_ver` = 2 且固件 `SCHEMA_VER` = 1
- **THEN** `load_group()` 返回 None 且 group 命名空间被清理

#### Scenario: NVS 读取失败时清理 group
- **WHEN** group 命名空间的 `my_priv_key` blob 长度不是 32 字节
- **THEN** `load_group()` 返回 None 且 group 命名空间被清理（调用 `clear_group`）

### Requirement: reset_settings 清空 sys
`reset_settings()` SHALL 清空 sys 命名空间全部键，使后续 `load_settings()` 返回默认值。

#### Scenario: 恢复出厂设置后 sys 回默认
- **WHEN** 调用 `reset_settings()` 后再调用 `load_settings()`
- **THEN** 返回的 Settings 所有字段为默认值

### Requirement: clear_group 清空 group
`clear_group()` SHALL 清空 group 命名空间全部键，使后续 `load_group()` 返回 None。

#### Scenario: 退出群组后 group 清空
- **WHEN** 调用 `clear_group()` 后再调用 `load_group()`
- **THEN** 返回 None

### Requirement: clear_diag 清空 diag
`clear_diag()` SHALL 清空 diag 命名空间全部键，使后续 `load_diag()` 返回默认值。

#### Scenario: 清空诊断后 diag 回默认
- **WHEN** 调用 `clear_diag()` 后再调用 `load_diag()`
- **THEN** 返回 `DiagInfo { abnormal_boot_cnt: 0, safe_boot_flag: false, last_reset_reason: 0 }`

### Requirement: 数据结构定义
项目 SHALL 定义以下数据结构：`Settings`（字段：`schema_ver: u16`、`device_name: String`、`volume: u8`、`muted: bool`、`brightness: u8`、`screen_off_sec: u32`）、`GroupInfo`（字段：`schema_ver: u16`、`my_priv_key: [u8; 32]`、`peers: Vec<PeerEntry>`、`mode: IntercomMode`、`channel: u8`、`last_state: u8`）、`PeerEntry`（字段：`mac: [u8; 6]`、`pub_key: [u8; 32]`）、`DiagInfo`（字段：`abnormal_boot_cnt: u32`、`safe_boot_flag: bool`、`last_reset_reason: u32`）、`StorageError` 枚举（变体：`Io`、`SchemaMismatch`、`Corrupt`）、`IntercomMode` 枚举（变体：`Clear`、`Free`）。字段定义 SHALL 与技术设计 §3.1 一致。`GroupInfo.last_state` 存储 last foreground App id，供 PRD §5.2 冷启动恢复使用。

#### Scenario: 结构体可构造与字段访问
- **WHEN** 在后续变更中构造 `Settings { schema_ver: 1, device_name: "INT-001".into(), volume: 50, muted: false, brightness: 80, screen_off_sec: 30 }`
- **THEN** 编译期通过且字段可被点号访问

### Requirement: PeerEntry 序列化上限
`PeerEntry` SHALL 序列化为 38 字节定长（6 字节 MAC + 32 字节公钥）。`GroupInfo.peers` 序列化后 SHALL 不超过 152 字节（4 节点上限），对齐 PRD §10.3 最大组容量 4。

#### Scenario: 4 节点 peers 序列化
- **WHEN** `GroupInfo.peers` 包含 4 个 PeerEntry 时序列化
- **THEN** 序列化结果长度 = 152 字节，可存入 NVS blob

#### Scenario: 超过 4 节点时拒绝
- **WHEN** `save_group` 收到 `peers.len() > 4` 的 GroupInfo
- **THEN** 返回 `Err(StorageError::Corrupt)`，不写入 NVS
