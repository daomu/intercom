## Why

17 个变更提案的第 3 个（依赖 change 01 的工程骨架）。当前骨架仅有 `BoardProfile` 编译期常量与空 `src/services/mod.rs`，没有任何运行时存储/密码学能力。对讲业务的冷启动恢复（§6.4）、组队三阶段 ECDH（§18.4）、设置持久化（PRD §6/§12）全部依赖 StorageService 与 CryptoService 两个底层 Service trait 才能落地。需要先定义 trait 契约、NVS 实现、curve25519-dalek 密钥派生实现，后续变更（change 07 设置应用、change 09 组队、change 12 冷启动恢复）才能在此之上构建业务。

## What Changes

- 新增 `src/services/storage/mod.rs`、`src/services/storage/nvs_storage.rs`：定义 `StorageService` trait 与 `NvsStorage` 实现
- 新增 `src/services/crypto/mod.rs`、`src/services/crypto/dalek_crypto.rs`：定义 `CryptoService` trait 与 `DalekCrypto` 实现
- 新增 `src/services/storage/types.rs`：`Settings`、`GroupInfo`、`PeerEntry`、`DiagInfo`、`StorageError`、`IntercomMode` 数据结构
- NVS 实现使用 3 个命名空间：`sys`（系统设置）/`group`（组信息）/`diag`（诊断信息），键名与类型遵循技术设计 §7.2-§7.4
- schema_ver 兼容性规则（§7.5）：==→加载；<固件→系统设置回退默认 + 组信息清理；>固件→组信息清理；读取失败/字段非法→组信息清理
- CryptoService 实现使用 `curve25519-dalek`：`gen_keypair`（X25519 私钥→公钥）、`derive_lmk`（X25519 标量乘 + HKDF-SHA256[salt="ESP32C6-INTERCOM", info="LMK-v1"][0..16]）、`validate_pubkey`（非全零且非 curve 上的低阶点）
- 不引入上层业务调用；仅 trait + 实现 + 数据结构，供后续变更引用

## Capabilities

### New Capabilities
- `storage-service`: 系统设置/组信息/诊断信息的 NVS 持久化能力，含 schema_ver 兼容性规则与损坏清理策略
- `crypto-service`: Curve25519 密钥对生成、X25519 ECDH + HKDF-SHA256 派生 16 字节 LMK、公钥合法性校验能力

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：新增 `src/services/storage/`、`src/services/crypto/` 两个子模块；`src/services/mod.rs` 注册子模块
- **依赖**：deps = `01`（change 01 工程骨架）。`curve25519-dalek`（已在 change 01 声明）正式调用；新增 `sha2`（HKDF-SHA256 依赖）、`hkdf`（HKDF 实现）或等价手写 HKDF
- **NVS**：使用 change 01 分区表中的 `nvs` 分区（0x9000/0x6000），3 个命名空间首次写入
- **后续变更**：
  - change 07（app-shell-settings）：消费 `StorageService::load_settings/save_settings/reset_settings`
  - change 09（pairing-three-phase）：消费 `CryptoService::gen_keypair/derive_lmk/validate_pubkey` + `StorageService::save_group`
  - change 12（restore-heartbeat）：消费 `StorageService::load_group` + `CryptoService::derive_lmk` 实现冷启动恢复
  - change 16（safety-diagnostics）：消费 `StorageService::inc_abnormal_boot/load_diag/clear_diag`
- **无运行时行为变更**：本期不创建 task、不接入 main 启动流程，仅提供可被后续变更实例化与调用的 trait + 实现
