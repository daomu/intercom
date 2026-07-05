## Why

17 个变更提案的第 1 个。当前仓库仅有 `Cargo.toml`/`build.rs`/`sdkconfig.defaults`/`rust-toolchain.toml`/`src/main.rs` 的占位骨架，无法编译也无法烧录出可见结果。需要先把它升级为"能 `cargo build` 通过、烧录后 LCD 点亮 slint 占位画面"的真实工程地基，后续所有变更（BSP 驱动、Service、对讲业务、UI）才能在此基础上增量扩展。

## What Changes

- 升级 `Cargo.toml`：单 crate，edition 2021，声明核心依赖（`esp-idf-svc`、`slint`、`curve25519-dalek`、`audiopus`、`embuild`、`embedded-graphics` 等），仅声明不调用业务逻辑
- 升级 `build.rs`：调用 `embuild::build::{self, esp_idf}` 处理 ESP-IDF 编译链接，并接入 slint 的 `slint_build::compile`
- 升级 `sdkconfig.defaults`：基础 ESP-IDF 配置——关闭 PSRAM、Flash 16MB、自定义分区表、Wi-Fi/ESP-NOW 启用、log 输出 UART
- 新增 `partitions.csv`：NVS / OTA Data / App A / App B / 资源区 预留布局
- 升级 `rust-toolchain.toml`：锁定 nightly + `esp` 工具链组件（esp-idf-svc std 要求）
- 新增 `.cargo/config.toml`：esp32c6 target、linker 配置、runner = `espflash`
- 新增 `src/board_profile.rs`：技术设计 §2 全部编译期常量（LCD 分辨率、引脚、最大组容量、发现信道、Opus 采样率/帧长、jitter 初始水位等）
- 新增 `ui/boot.slint`：最小 slint 文件，渲染全屏占位页面（"Intercom Boot OK" + 时间戳）
- 重写 `src/main.rs`：最小 `main`——引用 `BoardProfile` 常量做编译期校验、初始化 slint 后端、展示占位画面、阻塞循环

## Capabilities

### New Capabilities
- `project-layout`: 工程骨架的文件组织、构建配置、工具链锁定与编译期常量定义；不包含任何运行时业务逻辑

### Modified Capabilities
<!-- 无既有 spec，本期为首次引入 -->

## Impact

- **代码**：替换占位的 `src/main.rs`；新增 `src/board_profile.rs` 与 `ui/boot.slint`
- **构建**：`Cargo.toml` / `build.rs` / `sdkconfig.defaults` / `rust-toolchain.toml` / `.cargo/config.toml` 全部实质性升级
- **分区**：新增 `partitions.csv` 自定义分区表
- **依赖**：引入 `esp-idf-svc`、`slint`、`slint-build`、`curve25519-dalek`、`audiopus`、`embuild`、`embedded-graphics`、`anyhow`/`log`/`esp_idf_svc::log`（仅声明，业务调用在后续变更）
- **后续变更**：change 02-17 全部在此骨架上增量扩展；目录结构遵循技术设计 §20
- **无运行时行为变更**：本期仅为地基，不实现任何 BSP 驱动 / Service / 对讲业务
