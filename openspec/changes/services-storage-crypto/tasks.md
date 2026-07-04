## 1. 依赖与模块注册

- [ ] 1.1 在 `Cargo.toml` 的 `[dependencies]` 添加 `sha2 = "0.10"` 与 `hkdf = "0.12"`（`curve25519-dalek = "4"` 已在 change 01 声明）
- [ ] 1.2 在 `src/services/mod.rs` 注册 `pub mod storage;` 与 `pub mod crypto;`，移除占位 `#![allow(dead_code)]` 中对应条目
- [ ] 1.3 新增 `src/services/storage/mod.rs` 声明 `pub trait StorageService`、`pub use nvs_storage::NvsStorage;`、`pub use types::*;`
- [ ] 1.4 新增 `src/services/crypto/mod.rs` 声明 `pub trait CryptoService`、`pub use dalek_crypto::DalekCrypto;`、`pub struct KeyPair { ... }`

## 2. 数据结构定义

- [ ] 2.1 新增 `src/services/storage/types.rs`：定义 `Settings`（schema_ver/device_name/volume/muted/brightness/screen_off_sec）、`GroupInfo`（schema_ver/my_priv_key/peers/mode/channel/last_state）、`PeerEntry`（mac/pub_key）、`DiagInfo`（abnormal_boot_cnt/safe_boot_flag/last_reset_reason）、`StorageError`（Io/SchemaMismatch/Corrupt）、`IntercomMode`（Clear/Free）
- [ ] 2.2 为 `Settings` 实现 `Default`（device_name=随机名占位、volume=50、muted=false、brightness=80、screen_off_sec=30、schema_ver=SCHEMA_VER）
- [ ] 2.3 为 `DiagInfo` 实现 `Default`（全零/false）
- [ ] 2.4 为 `IntercomMode` 实现 `From<u8>` 与 `Into<u8>`（0=Clear, 1=Free）

## 3. CryptoService trait + DalekCrypto 实现

- [ ] 3.1 在 `src/services/crypto/mod.rs` 定义 `pub trait CryptoService: Send + Sync`，签名：`gen_keypair(&self) -> KeyPair`、`derive_lmk(&self, my_priv: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 16]`、`validate_pubkey(&self, pub_key: &[u8; 32]) -> bool`
- [ ] 3.2 新增 `src/services/crypto/dalek_crypto.rs`：定义 `pub struct DalekCrypto;` 与 `impl CryptoService for DalekCrypto`
- [ ] 3.3 实现 `gen_keypair`：`x25519::StaticSecret::random_from_rng(&mut OsRng)` → `priv_key`，`PublicKey::from(&secret)` → `pub_key`，返回 `KeyPair`
- [ ] 3.4 实现 `derive_lmk`：`StaticSecret::from(my_priv)` → `.diffie_hellman(&PublicKey::from(peer_pub))` → 32 字节 shared；`Hkdf::<Sha256>::new(b"ESP32C6-INTERCOM", &shared_bytes)` → `.expand(b"LMK-v1", &mut [0u8;16])` → 返回前 16 字节
- [ ] 3.5 实现 `validate_pubkey`：`pub_key != &[0u8; 32]`
- [ ] 3.6 验证 `curve25519-dalek` + `sha2` + `hkdf` 在 `riscv32imc-esp-espidf` target 下编译通过

## 4. StorageService trait + NvsStorage 实现

- [ ] 4.1 在 `src/services/storage/mod.rs` 定义 `pub trait StorageService: Send + Sync`，签名按技术设计 §3.1
- [ ] 4.2 新增 `src/services/storage/nvs_storage.rs`：定义 `pub struct NvsStorage { ... }`，持有 3 个命名空间句柄（`sys`/`group`/`diag`）
- [ ] 4.3 实现 `NvsStorage::new()`：调用 `nvs_flash_init()`（忽略已初始化错误），获取 3 个命名空间句柄，返回 `Result<Self, StorageError>`
- [ ] 4.4 实现 `load_settings()`：读 `sys` 各键，任一缺失/类型不匹配 → 返回 `Settings::default()`；schema_ver 检查按 §7.5
- [ ] 4.5 实现 `save_settings(&Settings)`：序列化为定长 blob 或逐键写入 NVS，返回 `Result<(), StorageError>`
- [ ] 4.6 实现 `reset_settings()`：擦除 `sys` 命名空间全部键
- [ ] 4.7 实现 `load_group()`：读 `group` 各键，schema_ver/读取失败/字段非法 → 调用 `clear_group()` 并返回 `None`；正常 → 反序列化为 `GroupInfo` 返回 `Some`
- [ ] 4.8 实现 `save_group(&GroupInfo)`：校验 `peers.len() <= 4`（否则返回 `Err(Corrupt)`），序列化 peers 为 4×38=152 字节 blob，逐键写入 NVS
- [ ] 4.9 实现 `clear_group()`：擦除 `group` 命名空间全部键
- [ ] 4.10 实现 `load_diag()`：读 `diag` 各键，缺失 → `DiagInfo::default()`
- [ ] 4.11 实现 `inc_abnormal_boot()`：读当前 `abnormal_boot_cnt` → +1 → 写回
- [ ] 4.12 实现 `clear_diag()`：擦除 `diag` 命名空间全部键

## 5. schema_ver 兼容性规则

- [ ] 5.1 定义编译期常量 `pub const SCHEMA_VER: u16 = 1;`（在 `src/services/storage/mod.rs` 或 `board_profile.rs`）
- [ ] 5.2 在 `load_settings()` 中：NVS `schema_ver` == `SCHEMA_VER` → 加载；`< SCHEMA_VER` → 回退默认 + 日志 warn；`> SCHEMA_VER` → 回退默认 + 日志 warn
- [ ] 5.3 在 `load_group()` 中：NVS `schema_ver` == `SCHEMA_VER` → 加载；`< SCHEMA_VER` → `clear_group()` + 返回 None；`> SCHEMA_VER` → `clear_group()` + 返回 None；读取失败/字段非法 → `clear_group()` + 返回 None

## 6. 序列化辅助

- [ ] 6.1 为 `PeerEntry` 实现 `to_bytes(&self) -> [u8; 38]` 与 `from_bytes(&[u8; 38]) -> Self`
- [ ] 6.2 为 `Vec<PeerEntry>` 实现 `to_blob(&self) -> Vec<u8>`（长度 = 38 × n）与 `from_blob(&[u8]) -> Result<Vec<PeerEntry>, StorageError>`（长度非 38 倍数 → Corrupt）
- [ ] 6.3 为 `GroupInfo` 实现内部序列化辅助：`my_priv_key` 直存 blob[32]，`mode`/`channel`/`last_state` 直存 u8，`peers` 用 `to_blob`

## 7. 单元测试（host 逻辑测试）

- [ ] 7.1 新增 `src/services/crypto/dalek_crypto.rs` 内 `#[cfg(test)]` 模块：测试 `derive_lmk` 对称性（A→B 与 B→A 一致）、`validate_pubkey` 全零拒绝、`gen_keypair` 两次不同
- [ ] 7.2 新增 `src/services/storage/types.rs` 内 `#[cfg(test)]` 模块：测试 `PeerEntry::to_bytes/from_bytes` 往返一致、`Vec<PeerEntry>::to_blob/from_blob` 长度校验
- [ ] 7.3 测试 schema_ver 规则逻辑（纯函数判定部分，不依赖真实 NVS）

## 8. 构建验证

- [ ] 8.1 执行 `cargo build`，确认 `curve25519-dalek`/`sha2`/`hkdf`/`esp-idf-svc` 全部编译通过
- [ ] 8.2 执行 `cargo build --release`，确认 release profile 通过（curve25519-dalek 首次 release 编译可能 10 分钟）
- [ ] 8.3 确认 `src/services/mod.rs` 注册子模块后无未使用警告（或保留 `#![allow(dead_code)]` 直到后续变更引用）

## 9. 收尾

- [ ] 9.1 提交 commit：`feat: add storage + crypto services (change 03/17)`，包含 `src/services/storage/`、`src/services/crypto/` 全部文件与 `Cargo.toml` 依赖更新
- [ ] 9.2 在 commit message 注明后续变更（07/09/12/16）将消费这两个 Service trait
