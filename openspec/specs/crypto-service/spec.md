## ADDED Requirements

### Requirement: CryptoService trait 定义
项目 SHALL 定义 `CryptoService` trait（`Send + Sync`），暴露以下方法签名：`gen_keypair(&self) -> KeyPair`、`derive_lmk(&self, my_priv: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 16]`、`validate_pubkey(&self, pub_key: &[u8; 32]) -> bool`。trait 签名 SHALL 与技术设计 §3.2 一致。

#### Scenario: trait 可被实现与引用
- **WHEN** 后续变更在 `src/services/crypto/mod.rs` 引用 `CryptoService` trait
- **THEN** 编译期解析到该 trait 定义，且任何实现该 trait 的类型可被多态调用

### Requirement: KeyPair 结构定义
项目 SHALL 定义 `KeyPair` 结构体，字段：`priv_key: [u8; 32]`、`pub_key: [u8; 32]`。字段定义 SHALL 与技术设计 §3.2 一致。

#### Scenario: gen_keypair 返回合法密钥对
- **WHEN** 调用 `gen_keypair()`
- **THEN** 返回的 `KeyPair` 中 `priv_key` 为 32 字节随机数，`pub_key` 为对应 X25519 公钥，且 `priv_key != [0u8; 32]`

### Requirement: DalekCrypto 实现
项目 SHALL 提供 `DalekCrypto` 结构体实现 `CryptoService` trait，使用 `curve25519-dalek` crate 的 `x25519` 模块。`DalekCrypto::new()` SHALL 返回 `Self` 且不 panic，不持有任何状态。

#### Scenario: 实例化 DalekCrypto
- **WHEN** 调用 `DalekCrypto::new()`
- **THEN** 返回 `DalekCrypto` 实例，可立即调用 trait 方法

### Requirement: gen_keypair 使用 X25519
`gen_keypair()` SHALL 使用 `curve25519-dalek::x25519::StaticSecret::random()`（或等价 API）生成 32 字节私钥，再通过 `PublicKey::from(&secret)` 推导公钥。私钥 SHALL 使用硬件随机源（ESP-IDF `esp_random`）。

#### Scenario: 同一密钥对私钥推导公钥一致
- **WHEN** 用私钥 `priv_key` 通过 X25519 推导公钥
- **THEN** 结果与 `gen_keypair()` 返回的 `pub_key` 一致

#### Scenario: 两次调用生成不同密钥对
- **WHEN** 连续调用 `gen_keypair()` 两次
- **THEN** 两次返回的 `priv_key` 不同（极大概率，2^-256 碰撞可忽略）

### Requirement: derive_lmk 使用 X25519 + HKDF-SHA256
`derive_lmk(my_priv, peer_pub)` SHALL 执行以下流程：X25519 标量乘 `shared = X25519(my_priv, peer_pub)` 得到 32 字节共享密钥；再通过 HKDF-SHA256（salt = `b"ESP32C6-INTERCOM"`，info = `b"LMK-v1"`）推导，取输出前 16 字节作为 LMK。流程 SHALL 与技术设计 §6.1 一致。

#### Scenario: 两节点对称推导 LMK
- **WHEN** 节点 A 用 `priv_A` × `pub_B` 调用 `derive_lmk`，节点 B 用 `priv_B` × `pub_A` 调用 `derive_lmk`
- **THEN** 两者返回的 16 字节 LMK 完全一致

#### Scenario: LMK 长度为 16 字节
- **WHEN** 调用 `derive_lmk(my_priv, peer_pub)`
- **THEN** 返回值类型为 `[u8; 16]`，长度恰好 16 字节，对齐 ESP-NOW 硬件 AES-CCM LMK 长度

#### Scenario: 不同对端推导不同 LMK
- **WHEN** 用同一 `my_priv` 与两个不同的 `peer_pub` 分别调用 `derive_lmk`
- **THEN** 两次返回的 LMK 不同（极大概率）

### Requirement: HKDF 参数固定
HKDF-SHA256 的 salt SHALL 为 `b"ESP32C6-INTERCOM"`（15 字节），info SHALL 为 `b"LMK-v1"`（6 字节）。这两个参数 SHALL 为编译期常量，不可在运行时修改。

#### Scenario: 相同输入确定性输出
- **WHEN** 用相同的 `(my_priv, peer_pub)` 多次调用 `derive_lmk`
- **THEN** 每次返回相同的 16 字节 LMK

### Requirement: validate_pubkey 拒绝全零公钥
`validate_pubkey(pub_key)` SHALL 在 `pub_key == [0u8; 32]` 时返回 `false`，否则返回 `true`。X25519 的 Montgomery ladder 对任意 32 字节输入均能运算，因此无需检查"点在曲线上"。

#### Scenario: 全零公钥被拒绝
- **WHEN** 调用 `validate_pubkey(&[0u8; 32])`
- **THEN** 返回 `false`

#### Scenario: 非零公钥被接受
- **WHEN** 调用 `validate_pubkey(&<非全零的32字节>)`
- **THEN** 返回 `true`

### Requirement: curve25519-dalek 编译通过
项目 SHALL 在 `riscv32imc-esp-espidf` target 下编译 `curve25519-dalek`、`sha2`、`hkdf` 依赖零错误通过。

#### Scenario: cargo build 通过
- **WHEN** 执行 `cargo build`
- **THEN** 编译退出码为 0，无链接错误，`DalekCrypto` 实现可被引用
