## Context

change 01 已落地工程骨架：`Cargo.toml` 声明 `curve25519-dalek = "4"`、`src/services/mod.rs` 为空 `mod.rs;` 占位，NVS 分区（0x9000/0x6000）已在 `partitions.csv` 预留。技术设计 §3.1/§3.2 给出 `StorageService` 与 `CryptoService` trait 签名，§7 给出 NVS 三命名空间（`sys`/`group`/`diag`）键名与类型，§6.1 给出 LMK 计算流程，§7.5 给出 schema_ver 兼容性规则表。本期在骨架之上落地这两个 Service trait + NVS 实现 + curve25519-dalek 实现，作为后续业务变更（07/09/12/16）的底层依赖。

## Goals / Non-Goals

**Goals:**
- `StorageService` trait + `NvsStorage` 实现就绪，可被后续变更 `let storage = NvsStorage::new()?;` 实例化
- `CryptoService` trait + `DalekCrypto` 实现就绪，可被后续变更 `let crypto = DalekCrypto::new();` 实例化
- `Settings`/`GroupInfo`/`PeerEntry`/`DiagInfo`/`StorageError`/`IntercomMode` 数据结构定义与 NVS 序列化/反序列化就绪
- schema_ver 兼容性规则按 §7.5 全部实现：==→load；<固件→settings 回退默认 + group cleanup；>固件→group cleanup；read fail→group cleanup
- LMK 派生 = X25519 标量乘 + HKDF-SHA256[salt="ESP32C6-INTERCOM", info="LMK-v1"][0..16]，输出 16 字节，对齐 ESP-NOW 硬件 AES-CCM LMK 长度
- `curve25519-dalek` 在 `riscv32imc-esp-espidf` target 下编译通过（首次编译可能 5-10 分钟）
- 单元可测性：纯逻辑函数（HKDF 截断、schema_ver 判定、序列化）可被 `#[cfg(test)]` 模块验证

**Non-Goals:**
- 在 `main.rs` 中实例化或调用这两个 Service → 后续变更按需引入
- 三 task 并发模型实际创建 → 后续变更
- 任何对讲业务逻辑（组队/冷启动恢复/PTT）→ change 09/12
- 设置 UI 页面 → change 07
- 诊断信息展示 UI → change 16
- 应用层加密存储（PRD §12.2 明确允许明文保存组信息与本机私钥）
- 跨设备 schema_ver 空中协议字段（§7.5 注：本期不保留空中协议版本字段）

## Decisions

### D1：NVS 实现使用 `embedded-svc::nvs::Nvs` 封装
esp-idf-svc 提供 `embedded-svc::nvs::Nvs`（基于 ESP-IDF `nvs_flash`），支持命名空间 + 键值读写。`NvsStorage` 持有 `Nvs` 实例，在 `new()` 中 `nvs_flash_init()` 后获取。备选：直接调用 `esp_idf_svc::nvs::*` 较底层 API——封装层已够用，无需暴露底层句柄，排除。

### D2：序列化 = 手写定长二进制（不引入 serde）
`Settings` 6 字段固定布局可编码为 ≤64 字节定长 blob；`GroupInfo` 的 `peers` 为 `Vec<PeerEntry>`，单 PeerEntry = 6+32=38 字节，上限 4 节点 = 152 字节。手写 `to_bytes()/from_bytes()` 避免 serde 派生在 no_std/esp-idf 边界的编译开销与二进制体积。备选：`serde` + `postcard`——体积/编译时间增加，且 NVS blob 大小有限，定长手写更可控，排除。

### D3：schema_ver 常量 = 编译期 `const SCHEMA_VER: u16 = 1;`
首次引入版本为 1。`sys` 与 `group` 各自独立保存 `schema_ver`，各自按 §7.5 规则判定。固件升级时此常量递增触发迁移逻辑。备选：运行时配置——无意义，版本应与固件绑定，排除。

### D4：CryptoService 使用 `curve25519-dalek` 的 `x25519` 模块
`x25519::EphemeralSecret`/`PublicKey` 封装了 RFC 7748 X25519 标量乘。但 `EphemeralSecret` 持有私钥且不可序列化，而 PRD §12.1 要求持久化本机私钥到 NVS。改用 `x25519::StaticSecret::from(priv_bytes)` → `.diffie_hellman(&peer_pub)` → `SharedSecret`，再喂给 HKDF。备选：手写 Curve25519 Montgomery ladder——不可接受的安全风险，排除。

### D5：HKDF-SHA256 = `hkdf` crate + `sha2` crate
`hkdf::Hkdf::<sha2::Sha256>` 提供 extract+expand 标准实现。salt = `b"ESP32C6-INTERCOM"`，info = `b"LMK-v1"`，输出取前 16 字节。备选：手写 HMAC-SHA256 + expand——增加审计面，无收益，排除。HKDF 输出 32 字节截断为 16 字节满足 ESP-NOW LMK 长度。

### D6：`validate_pubkey` = 非全零 + X25519 公钥解码不 panic
Curve25519 公钥在 X25519 中任何 32 字节都是合法的（RFC 7748 允许全空间），但全零公钥会导致 shared secret 全零，必须拒绝。`validate_pubkey` 检查 `pub_key != [0u8;32]` 即可。备选：检查"点在曲线上"——X25519 不要求点在曲线上（Montgomery ladder 自动处理 cofactor），此项检查无意义且误导，排除。

### D7：`StorageError` 枚举 = `Io`/`SchemaMismatch`/`Corrupt`
按技术设计 §3.1 定义。`Io` = NVS 读写底层错误；`SchemaMismatch` = schema_ver 不兼容（< 或 >）；`Corrupt` = 字段非法/反序列化失败。`SchemaMismatch` 与 `Corrupt` 在 group 命名空间均触发 cleanup，在 sys 命名空间均触发默认值回退。调用方可根据错误类型做日志区分。

### D8：`DiagInfo` 的 `abnormal_boot_cnt` 由调用方递增
`inc_abnormal_boot()` 实现 = 读当前值 → +1 → 写回。bootloader 钩子递增逻辑在 change 16（safety-diagnostics）实现，本期仅提供 NVS 读写能力。备选：本期内置 bootloader 钩子——超出本期 scope，排除。

### D9：`IntercomMode` 枚举 = `Clear`/`Free`
按技术设计 §3.5 定义，序列化为 u8（0=Clear, 1=Free）。`GroupInfo` 引用此枚举而非重新定义，避免类型重复。

### D10：`NvsStorage::new()` 幂等且 panic-free
多次调用 `nvs_flash_init()` 在 ESP-IDF 中安全（已初始化则返回已存在错误，可忽略）。`NvsStorage::new()` 返回 `Result<Self, StorageError>`，不 panic。备选：`new()` 返回 `Self` 并 panic on error——破坏错误传播链路，排除。

## Risks / Trade-offs

- **[curve25519-dalek 在 RISC-V 编译时间过长]** → 首次编译 5-10 分钟可接受；后续增量编译正常；CI 缓存 `target/` 目录
- **[NVS blob 大小限制（≤32KB/键）]** → `GroupInfo` 序列化后 ≤200 字节，远低于上限；`peers` 上限 4 节点 = 152 字节，无溢出风险
- **[NVS 磨损（erase 周期约 10 万次）]** → `save_settings`/`save_group` 调用频率低（用户改设置/组队时），磨损可忽略；`inc_abnormal_boot` 仅异常重启时调用
- **[明文存储私钥]** → PRD §12.2 明确允许本期明文存储；后续若需加密存储，可在 `NvsStorage` 内部加加密层而不破坏 trait 契约
- **[HKDF salt/info 硬编码]** → 版本化通过 `info="LMK-v1"` 承担；若未来需轮换 KDF 参数，递增 info 版本号即可，向后兼容
- **[schema_ver 迁移无空中协议字段]** → §7.5 明确不保留空中协议版本字段；跨设备组队时若双方 schema_ver 不一致，由 change 09 在 PAIR_JOIN_ACK 中拒绝加入

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证 `curve25519-dalek`/`sha2`/`hkdf` 编译通过
3. 后续变更（07/09/12/16）按需 `use crate::services::storage::NvsStorage;` 与 `use crate::services::crypto::DalekCrypto;`

回滚：`git revert <commit>`，回到空 `src/services/mod.rs` 占位状态。后续变更若已依赖这两个 Service，需一并回滚。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
