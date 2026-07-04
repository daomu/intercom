## Context

仓库当前是空骨架：`Cargo.toml` 仅 3 行、`build.rs` 空、`src/main.rs` 空、`sdkconfig.defaults` 占位、`rust-toolchain.toml` 仅工具链注释。无法 `cargo build`，更无法烧录出可见结果。

目标硬件：Waveshare ESP32-C6-Touch-LCD-1.54（ESP32-C6 RISC-V 单核 160MHz、512KB SRAM、16MB Flash、无 PSRAM、ST7789 240×240 LCD）。技术栈已与用户对齐：Rust + esp-idf-svc (std) + slint + audiopus + curve25519-dalek，单 crate，三 task 并发，2 台设备验收。

后续 16 个变更全部在此骨架上增量扩展，目录结构遵循技术设计 §20。

## Goals / Non-Goals

**Goals:**
- `cargo build` 在 `riscv32imc-esp-espidf` target 下零错误通过
- `cargo espflash` 烧录到目标设备后，上电点亮 LCD，显示 slint 渲染的占位画面（"Intercom Boot OK" + 启动时间戳）
- `BoardProfile` 编译期常量模块就绪，可被后续变更直接引用
- 工程目录与文件组织按技术设计 §20 落地（`src/services/`、`src/intercom/`、`src/apps/`、`src/hal/` 目录预留为空 `mod.rs`）
- 自定义分区表 `partitions.csv` 生效（NVS / OTA Data / App A / App B / 资源区）
- slint 编译期 `.slint` 文件接入 `build.rs`，生成 Rust 绑定代码

**Non-Goals:**
- 任何 BSP 驱动实现（ST7789/CST816/ES7210/ES8311/NS4150B/ESP-NOW/ADC/按键）→ change 02
- 任何 Service trait 或实现 → change 03-06
- 任何对讲业务逻辑、包格式、状态机 → change 08+
- 真实 UI 页面（对讲页/设置页/Launcher）→ change 07/13
- 三 task 并发的实际创建 → 后续变更按需创建，本期仅在文档中固化决策
- 任何单元测试或 on-device 集成测试

## Decisions

### D1：工具链 = nightly + `esp` component
esp-idf-svc 的 std 支持要求 nightly Rust + `rust-src` + `esp` 工具链覆盖。`rust-toolchain.toml` 锁定 channel = nightly，components = `rust-src`，targets = `riscv32imc-esp-espidf`。备选：stable + esp-idf-svc 的非 std 模式——但 PRD 已选 std 路径，排除。

### D2：构建集成 = `embuild` + `slint_build`
`build.rs` 顺序：先 `slint_build::compile("ui/boot.slint")` 生成 Rust 绑定，再 `embuild::build::esp_idf()` 处理 ESP-IDF。两者独立，无顺序耦合风险，但先 slint 可避免 embuild 早期 panic 中断 slint 生成。备选：`built` 工具——无 esp-idf 集成，排除。

### D3：slint 后端 = interpreter 模式（编译期 `.slint` → Rust 绑定）
使用 `slint_build::compile` 在编译期生成 `slint_generated_*` 模块。运行时用默认后端（`slint::default::default()`），不引入 slint-compiler 二进制依赖。备选：纯 interpreter `slint::interpreter`——编译期检查丢失，排除。

### D4：分区表 = 自定义 `partitions.csv`
PRD §25/技术 §18 要求预留 OTA 双区。布局（16MB Flash）：
```
nvs,      data, nvs,     0x9000,  0x6000
otadata,  data, ota,     0xf000,  0x8000
ota_0,    app,  ota_0,   0x10000, 0x700000
ota_1,    app,  ota_1,   0x710000,0x700000
resource, data, 0x40,    0xE10000,0x1F0000
```
注：`nvs`/`otadata`/`resource` 为 data 分区，offset 不必与 app 分区一样对齐 0x10000；`otadata` 的 0xf000 是 ESP-IDF 标准位置，免于 0x10000 对齐约束（app 分区 `ota_0`/`ota_1` 仍严格对齐 0x10000）。
备选：默认分区表——无 OTA 预留，违反 PRD §25，排除。

### D5：`BoardProfile` = 纯 `const` 常量结构（非 generic）
技术设计 §2 已给出 `pub struct BoardProfile; impl BoardProfile { pub const LCD_W: u32 = 240; ... }`。零运行时实例化，编译期常量直接引用。备选：`const generics`——过度工程化，排除。

### D6：依赖版本 = 不锁死 minor，仅锁 major
`Cargo.toml` 用 `esp-idf-svc = "0.49"`、`slint = "1.6"`、`curve25519-dalek = "4"`、`audiopus = "0.3"`、`embuild = "0.32"` 等。`Cargo.lock` 锁定具体版本。备选：`=x.y.z` 精确锁定——后续变更升级困难，排除。

### D7：目录结构 = 技术设计 §20 预创建空 `mod.rs`
为避免后续变更频繁创建目录，本期一次性预创建 `src/services/mod.rs`、`src/intercom/mod.rs`、`src/apps/mod.rs`、`src/hal/mod.rs`（均为空 `mod.rs;` 占位）。`main.rs` 仅 `mod board_profile;` 引用，其余模块在后续变更中按需注册。备选：按需创建——多变更频繁新增目录，git diff 噪声大，排除。

### D8：日志 = `esp_idf_svc::log` + `log` crate
启动时 `EspLogger::initialize_default()`，输出到 UART。本期 `main.rs` 打印一行 `info!("Intercom boot OK")`。备选：`defmt`——ESP32-C6 RISC-V 支持不成熟，排除。

### D9：slint 占位画面 = 单 `boot.slint` 文件
`ui/boot.slint` 定义一个全屏 `Window`，垂直居中显示 `Text { "Intercom Boot OK" }` + 第二行 `Text` 显示 `BoardProfile::LCD_W` × `LCD_H`（通过 `slint::shared` 传递）。备选：多 `.slint` 文件——本期无必要，排除。

### D10：三 task 并发决策固化（文档级，不实现）
本期 `main.rs` 只在 slint 主循环阻塞。三 task（采集编码发送 / 接收解码播放 / UI 业务）的优先级与栈大小在 `design.md` 此处记录，后续变更创建时遵循：
- Task A 音频实时：优先级 19（最高），栈 8KB
- Task B 网络业务：优先级 12，栈 6KB
- Task C UI/Shell：优先级 5，栈 12KB（slint 渲染栈较深）

## Risks / Trade-offs

- **[slint 在无 PSRAM ESP32-C6 上的渲染性能]** → 本期仅静态文字占位画面，无性能压力；后续变更若 UI 复杂度上升卡顿，则降级为 `embedded-graphics` 自绘（已在 D3 留出退出口：`slint` 与 `DisplayService::present` 解耦）
- **[esp-idf-svc 与 nightly 工具链漂移]** → `rust-toolchain.toml` 锁定具体 nightly 日期（`nightly-2026-XX-XX`）避免随机 CI 失败；后续变更升级时统一更新
- **[audiopus FFI 链接 libopus 静态库]** → 本期仅声明依赖不调用；change 05 实际集成时若链接失败，则改用 `opus-rs` 或自编译 libopus fixed-point
- **[curve25519-dalek 在 RISC-V 的编译时间过长]** → 本期仅声明依赖；首次 `cargo build` 可能 5-10 分钟，后续增量编译正常
- **[slint_build 与 embuild 共存时 build.rs panic]** → D2 已通过顺序约束缓解；若仍失败，拆为 `build.rs` + `build_slint.rs` 两阶段
- **[自定义分区表与 ESP-IDF 默认 bootloader 不兼容]** → 使用标准分区类型（`nvs`/`ota`/`app`/`data`），不引入自定义类型；offset 对齐 0x10000
- **[预创建空 `mod.rs` 导致未使用警告]** → 各 `mod.rs` 加 `#![allow(dead_code)]` 或 `#![allow(unused)]`；后续变更按需移除

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 拉取本变更 commit
2. `rustup toolchain install nightly` + `rustup target add riscv32imc-esp-espidf`
3. 安装 ESP-IDF v5.x（embuild 自动探测 `IDF_PATH` 或 `~/.espressif/`）
4. `cargo build` 验证编译
5. `cargo espflash flash --port /dev/ttyUSB0` 烧录
6. 上电观察 LCD

回滚：`git revert <commit>`，回到空骨架状态。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。
