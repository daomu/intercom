## 1. 依赖声明与模块注册

- [x] 1.1 在 `Cargo.toml` 的 `[dependencies]` 添加 `sha2 = "0.10"` 与 `hkdf = "0.12"`（`curve25519-dalek = "4"` 已在 change 01 声明）
- [x] 1.2 在 `src/services/mod.rs` 注册 `pub mod storage;` 与 `pub mod crypto;`，移除占位 `#![allow(dead_code)]` 中对应条目
- [x] 1.3 新增 `src/services/storage/mod.rs` 声明 `pub trait StorageService`、`pub use nvs_storage::NvsStorage;`、`pub use types::*;`
- [x] 1.4 新增 `src/services/crypto/mod.rs` 声明 `pub trait CryptoService`、`pub use dalek_crypto::DalekCrypto;`、`pub struct KeyPair`

## 2. 数据结构定义

- [x] 2.1 新增 `src/services/storage/types.rs`：定义 `Settings`（schema_ver/device_name/volume/muted/brightness/screen_off_sec）、`GroupInfo`、`PeerEntry`、`DiagInfo`、`StorageError`、`IntercomMode`
- [x] 2.2 为 `Settings` 实现 `Default`（device_name=随机名占位、volume=50、muted=false、brightness=80、screen_off_sec=30、schema_ver=SCHEMA_VER）
- [x] 2.3 为 `DiagInfo` 实现 `Default`（全零/false）
- [x] 2.4 为 `IntercomMode` 实现 `From<u8>` 与 `Into<u8>`（0=Clear, 1=Free）

## 3. CryptoService

- [x] 3.1 在 `src/services/crypto/mod.rs` 定义 `pub trait CryptoService: Send + Sync`，签名：`gen_keypair(&self) -> KeyPair`、`derive_lmk(&self, my_priv: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 16]`、`validate_pubkey(&self, pub_key: &[u8; 32]) -> bool`
- [x] 3.2 新增 `src/services/crypto/dalek_crypto.rs`：定义 `pub struct DalekCrypto;` 与 `impl CryptoService for DalekCrypto`
- [x] 3.3 实现 `gen_keypair`：`StaticSecret::random_from_rng(&mut EspRng)` → `priv_key`，`PublicKey::from(&secret)` → `pub_key`
- [x] 3.4 实现 `derive_lmk`：`StaticSecret::from(my_priv)` → `.diffie_hellman(&PublicKey::from(peer_pub))` → 32 字节 shared；`Hkdf::new(salt, &shared).expand(info, &mut okm)`；取 okm[0..16]
- [x] 3.5 实现 `validate_pubkey`：`pub_key != &[0u8; 32]`
- [x] 3.6 验证 `curve25519-dalek` + `sha2` + `hkdf` 在 `riscv32imac-esp-espidf` target 下编译通过

## 4. StorageService

- [x] 4.1 在 `src/services/storage/mod.rs` 定义 `pub trait StorageService: Send + Sync`，签名按技术设计 §3.1
- [x] 4.2 新增 `src/services/storage/nvs_storage.rs`：定义 `pub struct NvsStorage { ... }`，持有 3 个命名空间句柄（`sys`/`group`/`diag`）
- [x] 4.3 实现 `NvsStorage::new()`：调用 `nvs_flash_init()`（忽略已初始化错误），获取 3 个命名空间句柄，返回 `Result<Self, StorageError>`
- [x] 4.4 实现 `load_settings()`：读 `sys` 各键，任一缺失/类型不匹配 → 返回 `Settings::default()`；schema_ver 检查按 §7.5
- [x] 4.5 实现 `save_settings(&Settings)`：序列化为定长 blob 或逐键写入 NVS，返回 `Result<(), StorageError>`
- [x] 4.6 实现 `reset_settings()`：擦除 `sys` 命名空间全部键
- [x] 4.7 实现 `load_group()`：读 `group` 各键，schema_ver/读取失败/字段非法 → 调用 `clear_group()` 并返回 `None`；正常 → 反序列化为 `GroupInfo` 返回 `Some`
- [x] 4.8 实现 `save_group(&GroupInfo)`：校验 `peers.len() <= 4`（否则返回 `Err(Corrupt)`），序列化 peers 为 4×38=152 字节 blob，逐键写入 NVS
- [x] 4.9 实现 `clear_group()`：擦除 `group` 命名空间全部键
- [x] 4.10 实现 `load_diag()`：读 `diag` 各键，缺失 → `DiagInfo::default()`
- [x] 4.11 实现 `inc_abnormal_boot()`：读当前 `abnormal_boot_cnt` → +1 → 写回
- [x] 4.12 实现 `clear_diag()`：擦除 `diag` 命名空间全部键

## 5. schema_ver 兼容性规则

- [x] 5.1 定义编译期常量 `pub const SCHEMA_VER: u16 = 1;`（在 `src/services/storage/types.rs`）
- [x] 5.2 在 `load_settings()` 中：NVS `schema_ver` == `SCHEMA_VER` → 加载；`< SCHEMA_VER` → 回退默认 + 日志 warn；`> SCHEMA_VER` → 回退默认 + 日志 warn
- [x] 5.3 在 `load_group()` 中：NVS `schema_ver` == `SCHEMA_VER` → 加载；`< SCHEMA_VER` → `clear_group()` + 返回 None；`> SCHEMA_VER` → `clear_group()` + 返回 None

## 6. 序列化辅助

- [x] 6.1 为 `PeerEntry` 实现 `to_bytes(&self) -> [u8; 38]` 与 `from_bytes(&[u8; 38]) -> Self`
- [x] 6.2 为 `Vec<PeerEntry>` 实现 `to_blob(&self) -> Vec<u8>`（长度 = 38 × n）与 `from_blob(&[u8]) -> Result<Vec<PeerEntry>, StorageError>`
- [x] 6.3 为 `GroupInfo` 实现内部序列化辅助：`my_priv_key` 直存 blob[32]，`mode`/`channel`/`last_state` 直存 u8，`peers` 用 `to_blob`

## 7. 单元测试

- [x] 7.1 新增 `src/services/crypto/dalek_crypto.rs` 内 `#[cfg(test)]` 模块：测试 `derive_lmk` 对称性（A→B 与 B→A 一致）、`validate_pubkey` 全零拒绝、`gen_keypair` 两次不同
- [x] 7.2 新增 `src/services/storage/types.rs` 内 `#[cfg(test)]` 模块：测试 `PeerEntry::to_bytes/from_bytes` 往返一致、`Vec<PeerEntry>::to_blob/from_blob` 往返一致
- [x] 7.3 测试 schema_ver 规则逻辑（纯函数判定部分，不依赖真实 NVS）

## 8. 构建验证

- [x] 8.1 执行 `cargo build`，确认 `curve25519-dalek`/`sha2`/`hkdf`/`esp-idf-svc` 全部编译通过
- [x] 8.2 执行 `cargo build --release`，确认 release profile 通过
- [x] 8.3 确认 `src/services/mod.rs` 注册子模块后无未使用警告（保留 `#![allow(dead_code)]` + `#![allow(unused_imports)]` 直到后续变更引用）

## 9. 收尾

- [x] 9.1 提交 commit：`feat: add storage + crypto services (change 03/17)`，包含 `src/services/storage/`、`src/services/crypto/` 全部文件
- [x] 9.2 在 commit message 注明后续变更（07/09/12/16）将消费这两个 Service trait

---

## 实施偏差记录（Errata）

- **1.1 / 3.3 / 3.6 crate 切换：x25519-dalek 替代 curve25519-dalek::x25519**：design D4 原写「`curve25519-dalek` 的 `x25519` 模块」，但 `curve25519-dalek` 4.x 已移除 `x25519` 子模块（features 仅 `alloc`/`default`/`digest`/`legacy_compatibility`/`precomputed-tables`/`rand_core`/`serde`/`zeroize`）。改用独立维护的 `x25519-dalek = "2"` crate（同一底层 Curve25519 Montgomery ladder，RFC 7748 兼容）。`curve25519-dalek = "4"` 保留用于其 `rand_core` trait re-export。

- **3.3 / 3.6 EspRng 替代 OsRng**：x25519-dalek 2.0.1 的 `StaticSecret::random_from_rng` 需要 `R: RngCore + CryptoRng`。`rand_core::OsRng` 在 `target_os = "espidf"` 上依赖 `getrandom` crate，而 `getrandom` 对 espidf 后端的支持不确定。新增 `EspRng` 结构体实现 `RngCore + CryptoRng`，`fill_bytes` 内调用 `esp_idf_sys::esp_random()`（ESP32-C6 硬件 RNG，bootloader 启动时由 RF 噪声播种）。在 Cargo.toml 显式声明 `rand_core = { version = "0.6", default-features = false }` 仅取 `RngCore`/`CryptoRng` trait。

- **4.3 NvsStorage::new() 用 EspNvsPartition::take()**：design D1 原写「`embedded-svc::nvs::Nvs` 封装」，实际 esp-idf-svc 0.52.1 + git-main 提供 `EspNvsPartition<NvsDefault>` + `EspNvs::new(partition, namespace, read_write)`。`EspNvsPartition::take()` 内部调用 `nvs_flash_init()` 并处理 `ESP_ERR_NVS_NO_FREE_PAGES` / `ESP_ERR_NVS_NEW_VERSION_FOUND` 自动 reinit。`NvsStorage` 持有 `EspNvsPartition<NvsDefault>`（内部 Arc，廉价 clone），每次 `open(ns)` 创建新的 `EspNvs::new(partition.clone(), ns, true)` 句柄——避免跨 task 共享 Mutex。

- **4.7 / 5.3 read_blob 返回 `Option<&[u8]>` 需显式 lifetime**：`fn read_blob<'a>(nvs: &EspDefaultNvs, key: &str, buf: &'a mut [u8]) -> Option<&'a [u8]>`，借用必须绑定到 buf 而非 nvs（EspNvs 的 get_blob 返回的切片借自调用方提供的 buf）。

- **8.3 services/mod.rs 加 `#![allow(unused_imports)]`**：`pub use dalek_crypto::DalekCrypto`、`pub use nvs_storage::NvsStorage` 等导出在 change 03 落地后无消费方（main.rs 不实例化），但 re-export 必须保留以供 change 07/09/12/16 引用。除既有 `#![allow(dead_code)]` 外增 `#![allow(unused_imports)]` 抑制 warning。

- **NVS 数据布局与 schema_ver 规则**：spec §7.5 的规则已实现为纯函数 `apply_schema_rule(nvs_ver, fw_ver) -> SchemaAction { Load | Fallback | Cleanup }`（types.rs），便于 `#[cfg(test)]` 单元测试不依赖真实 NVS。`load_settings` / `load_group` 调用此函数；`Fallback` 与 `Cleanup` 在 sys 命名空间回退默认 Settings，在 group 命名空间调用 `clear_group()` 并返回 None。

- **明文存储私钥**：PRD §12.2 明确允许本期明文存储 `my_priv_key`；`NvsStorage::save_group` 直接 `set_blob("my_priv_key", &g.my_priv_key)` 写入 NVS，不加密。后续若需加密存储，可在 `NvsStorage` 内部加加密层而不破坏 `StorageService` trait 契约。
