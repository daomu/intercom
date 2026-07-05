## ADDED Requirements

### Requirement: 工程可编译
项目 SHALL 在 `riscv32imc-esp-espidf` target 下执行 `cargo build` 零错误通过；`Cargo.lock` SHALL 提交到版本控制以保证可复现构建。

#### Scenario: 全新克隆首次构建
- **WHEN** 在装有 ESP-IDF v5.x 与 nightly Rust 工具链的环境下执行 `cargo build`
- **THEN** 命令退出码为 0，无编译错误，仅有第三方 crate 的可选警告

#### Scenario: 无 ESP-IDF 环境的失败提示
- **WHEN** 在未安装 ESP-IDF 或未设置 `IDF_PATH` 的环境下执行 `cargo build`
- **THEN** `embuild` 输出明确的错误信息指出 ESP-IDF 未找到，不产生误导性的链接错误

### Requirement: 工具链锁定
项目根目录 SHALL 包含 `rust-toolchain.toml`，锁定 nightly channel、`rust-src` component 与 `riscv32imc-esp-espidf` target。

#### Scenario: 工具链自动应用
- **WHEN** 在仓库目录下执行任何 `cargo` 命令
- **THEN** rustup 自动使用 `rust-toolchain.toml` 指定的工具链，无需手动 `rustup override`

### Requirement: 构建脚本集成
`build.rs` SHALL 调用 `slint_build::compile` 编译 `.slint` 文件生成 Rust 绑定，并调用 `embuild::build::esp_idf` 接入 ESP-IDF 构建系统。

#### Scenario: slint 文件变更触发重编译
- **WHEN** 修改 `ui/boot.slint` 后执行 `cargo build`
- **THEN** `slint_build::compile` 重新生成绑定代码，主 crate 重新编译

#### Scenario: ESP-IDF 配置变更生效
- **WHEN** 修改 `sdkconfig.defaults` 后执行 `cargo build`
- **THEN** `embuild` 读取新配置并应用到 ESP-IDF 构建

### Requirement: 自定义分区表
项目 SHALL 包含 `partitions.csv`，定义 NVS / OTA Data / App A / App B / 资源区 五个分区，Flash 容量 16MB，所有分区 offset 对齐 0x10000。

#### Scenario: 分区表被 sdkconfig 引用
- **WHEN** 构建固件
- **THEN** `sdkconfig.defaults` 中 `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME` 指向 `partitions.csv`，生成的 bootloader 按此表分区

#### Scenario: OTA 双区可用
- **WHEN** 检查分区表
- **THEN** `ota_0` 与 `ota_1` 两个 app 分区容量相等，足以容纳固件镜像

### Requirement: 编译期常量模块
项目 SHALL 提供 `src/board_profile.rs` 暴露 `BoardProfile` 结构，包含技术设计 §2 列出的全部编译期常量：LCD 分辨率、背光支持、默认亮度、默认熄屏时间、单麦/单声道能力、是否真关机、电池 ADC 引脚、PA 控制引脚、背光引脚、BOOT/PLUS 按键引脚、最大组容量、发现信道、Opus 采样率/帧长、jitter 初始帧数。

#### Scenario: 常量可被引用
- **WHEN** 在 `src/main.rs` 中引用 `board_profile::BoardProfile::LCD_W`
- **THEN** 编译期解析为 `240u32`，无运行时开销

#### Scenario: 引脚常量与硬件一致
- **WHEN** 检查 `BOOT_BTN_PIN` / `PLUS_BTN_PIN` / `BAT_ADC_PIN` / `PA_CTRL_PIN` / `BACKLIGHT_PIN`
- **THEN** 值分别为 9 / 18 / 0 / 15 / 6，与 Waveshare ESP32-C6-Touch-LCD-1.54 原理图一致

### Requirement: 目录结构预留
项目 SHALL 预创建技术设计 §20 列出的目录与空 `mod.rs`：`src/services/`、`src/intercom/`、`src/apps/`、`src/hal/`，每个 `mod.rs` 内部允许 `#![allow(dead_code)]` 占位。

#### Scenario: 后续变更无需新建顶层目录
- **WHEN** change 02 在 `src/hal/` 下添加 `bsp.rs`
- **THEN** 仅需在 `src/hal/mod.rs` 追加 `pub mod bsp;`，无需创建新目录

### Requirement: slint 占位画面
项目 SHALL 包含 `ui/boot.slint`，定义全屏 `Window`，渲染居中的 "Intercom Boot OK" 文字；`src/main.rs` SHALL 初始化 slint 后端并阻塞主循环显示该画面。

#### Scenario: 设备上电显示占位画面
- **WHEN** 烧录固件后给 Waveshare ESP32-C6-Touch-LCD-1.54 上电
- **THEN** LCD 背光点亮，屏幕居中显示 "Intercom Boot OK" 文字，画面持续显示直到断电

#### Scenario: 编译期 slint 语法校验
- **WHEN** `ui/boot.slint` 存在语法错误时执行 `cargo build`
- **THEN** `slint_build::compile` 输出错误位置与原因，构建失败

### Requirement: 日志输出
`src/main.rs` SHALL 在 `main` 函数入口初始化 `esp_idf_svc::log::EspLogger`，并打印至少一行 `info!` 日志表明启动完成。

#### Scenario: UART 日志可见
- **WHEN** 通过 USB 连接设备并打开串口监视器（115200 8N1）
- **THEN** 上电后可见 "Intercom boot OK" 级别 INFO 的日志输出

### Requirement: 三 task 并发模型决策固化
`design.md` SHALL 记录三 task 并发模型的优先级与栈大小约定：Task A 音频实时（优先级 19 / 栈 8KB）、Task B 网络业务（优先级 12 / 栈 6KB）、Task C UI/Shell（优先级 5 / 栈 12KB）。本变更 SHALL NOT 实际创建这些 task，仅在文档中固化决策供后续变更遵循。

#### Scenario: 后续变更遵循约定
- **WHEN** change 05 创建音频采集 task
- **THEN** 该 task 的优先级与栈大小与本变更 `design.md` 记录的 Task A 约定一致
