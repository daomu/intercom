## 1. 工具链与构建配置

- [x] 1.1 更新 `rust-toolchain.toml`：channel = `nightly`（active = nightly-2026-06-22），components = `["rust-src"]`，targets = `["riscv32imac-esp-espidf"]`（ESP32-C6 IMAC；tasks 原文 `imc` 为笔误，已按硬件正确 target 落地）
- [x] 1.2 更新 `.cargo/config.toml`：`[build] target = "riscv32imac-esp-espidf"`，`[env] ESP_IDF_VERSION = "v5.5.3"`（满足 5.x），`[net] retry = 3`，runner = `espflash flash --monitor`
- [x] 1.3 更新 `sdkconfig.defaults`：关闭 PSRAM（未设置 `CONFIG_SPIRAM`）、Flash 16MB（`CONFIG_ESPTOOLPY_FLASHSIZE_16MB=y`）、自定义分区表（`CONFIG_PARTITION_TABLE_CUSTOM=y` + `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME="partitions.csv"`）、Wi-Fi/ESP-NOW 启用、log 输出 UART INFO（`CONFIG_LOG_DEFAULT_LEVEL_INFO=y`）
- [x] 1.4 新增 `partitions.csv`：nvs(0x9000/0x6000) · otadata(0xf000/0x8000) · ota_0(0x10000/0x700000) · ota_1(0x710000/0x700000) · resource(0xE10000/0x1F0000)；app 分区 offset 对齐 0x10000，data 分区（nvs/otadata/resource）按 ESP-IDF 标准免于该对齐约束（见 design.md D4 注）

## 2. Cargo 依赖声明

- [x] 2.1 更新 `Cargo.toml`：`[package]` 名称 `intercom`、版本 `0.1.0`、edition `2021`、`resolver=2`；`[[bin]] name = "intercom" path = "src/main.rs"`
- [x] 2.2 添加 `[dependencies]`：`esp-idf-svc = { version = "0.52.1", features = ["critical-section", "embassy-time-driver", "embassy-sync"] }`（按 design D6 minor 灵活，0.52.1 > tasks 原 0.49，同 major 0，保持模板可用版本）、`slint = { version = "1.6", default-features = false, features = ["compat-1-2","std","backend-esp-idf"] }`（esp-idf 后端）、`curve25519-dalek = "4"`、`audiopus = "0.3"`、`embedded-graphics = "0.8"`、`anyhow = "1"`、`log = "0.4"`、`embassy-time = "0.5"`
- [x] 2.3 添加 `[build-dependencies]`：`slint-build = "1.6"`、`embuild = "0.33"`
- [x] 2.4 添加 `[profile.release]`：`opt-level = "s"`、`lto = true`、`codegen-units = 1`

## 3. 构建脚本

- [x] 3.1 重写 `build.rs`：先 `slint_build::compile("ui/boot.slint").expect(...)`，再 `embuild::espidf::sysenv::output()`；输出 `BUILD_TIME` env 供 main `env!("BUILD_TIME")` 引用（`DEP_INTERCOM_BUILD_TIME` 前缀仅适用于 build-dep 互引，自 crate 用 rustc-env=BUILD_TIME 等效）
- [x] 3.2 验证 slint 编译期生成 `slint_generated_boot` 模块路径与 `main.rs` 的 `use` 路径一致（main.rs 用 `slint::include_modules!()` 生成 `BootWindow`）

## 4. BoardProfile 模块

- [x] 4.1 新增 `src/board_profile.rs`：定义 `pub struct BoardProfile;` 与 `impl BoardProfile`，全部 `pub const` 常量按技术设计 §2 落地（LCD_W=240、LCD_H=240、BACKLIGHT_PWM_SUPPORTED=true、DEFAULT_BRIGHTNESS=80、DEFAULT_SCREEN_OFF_SEC=30、MIC_CHANNELS=1、SPEAKER_CHANNELS=1、SUPPORTS_TRUE_POWER_OFF=false、BAT_ADC_PIN=0、PA_CTRL_PIN=15、BACKLIGHT_PIN=6、BOOT_BTN_PIN=9、PLUS_BTN_PIN=18、PWR_BTN_PIN=7、MAX_GROUP_SIZE=4、DISCOVERY_CHANNEL=1、OPUS_SAMPLE_RATE=16000、OPUS_FRAME_MS=20、JITTER_INIT_FRAMES=3、BAT_ADC_DIVIDER=3）
- [x] 4.2 在 `src/main.rs` 顶部 `mod board_profile;` 注册模块

## 5. 目录结构预留

- [x] 5.1 新增 `src/services/mod.rs`：`#![allow(dead_code)]` 占位
- [x] 5.2 新增 `src/intercom/mod.rs`：`#![allow(dead_code)]` 占位
- [x] 5.3 新增 `src/apps/mod.rs`：`#![allow(dead_code)]` 占位
- [x] 5.4 新增 `src/hal/mod.rs`：`#![allow(dead_code)]` 占位
- [x] 5.5 在 `src/main.rs` 用 `mod services; mod intercom; mod apps; mod hal;` 注册（编译期占位通过即可，无运行时调用）

## 6. slint 占位画面

- [x] 6.1 新增 `ui/boot.slint`：`export component BootWindow inherits Window`，全屏深色背景（#101418），垂直居中两行 `Text`——"Intercom Boot OK" + "240 × 240"
- [x] 6.2 在 `src/main.rs` 引用 `slint::include_modules!()` 生成 `BootWindow`，构造后调用 `.show()` 与 `.run()` 阻塞

## 7. main.rs 入口

- [x] 7.1 重写 `src/main.rs`：`fn main() -> anyhow::Result<()>`，`EspLogger::initialize_default()`，`info!("Intercom boot OK, LCD {}×{}, build={}", BoardProfile::LCD_W, BoardProfile::LCD_H, env!("BUILD_TIME"))`，构造 `BootWindow` 并 `.show().run()` 阻塞
- [x] 7.2 在 main 末尾 `Ok(())` 返回，处理 slint run 退出

## 8. 构建验证

- [x] 8.1 执行 `cargo build`，确认零编译错误（第三方 crate 警告可接受）
- [x] 8.2 执行 `cargo build --release`，确认 release profile 通过
- [x] 8.3 提交 `Cargo.lock` 到版本控制

## 9. 烧录验收

（见文末「9. 烧录验收（硬件任务，需物理设备）」——9.1–9.4 需物理设备，本次会话无硬件）

## 10. 收尾

- [x] 10.1 在仓库根目录提交 commit：`feat: bootstrap project skeleton (change 01/17)`，包含全部骨架文件
- [x] 10.2 在 commit message 注明后续变更将在此骨架上增量扩展，引用技术设计 §20 与本变更的 design.md

---

## 实施偏差记录（Errata）

实施过程中发现 design.md / tasks.md 原文与 ESP-IDF + slint 实际环境存在出入，已就地修正并在此记录，供后续变更追溯：

- **1.4 分区表重叠修正**：design.md D4 / tasks.md 1.4 原写 `otadata 0xf000/0x8000`，与 `ota_0 0x10000` 重叠（otadata 结束于 0x17000 > 0x10000），ESP-IDF `gen_esp32part.py` 拒绝。修正为 `nvs 0x9000/0x5000`、`otadata 0xe000/0x2000`（8KB，ESP-IDF 双 esp_ota_select_entry 最小值），data 分区仍免 0x10000 对齐约束（D4 注）。
- **2.2 audiopus 改为 optional + pin 0.3.0-rc.0**：crates.io 上 `audiopus = "0.3"` 无稳定版，仅 `0.3.0-rc.0` 预发布（D6 "仅锁 major" 放宽至 0.3 系列）。又 `audiopus_sys` 的 build.rs 在找不到系统 libopus 时用 cmake 从源码编译，需要 `riscv32-esp-elf-gcc` 在 PATH，而 esp-idf-sys 构建子进程的 PATH 不会传递给 audiopus_sys 的 build script。故 change 01 将 audiopus 声明为 `optional = true`（feature `opus`，默认关闭），libopus 交叉编译接线（`LIBOPUS_LIB_DIR` 或工具链 PATH）留待 change 05。
- **2.2 slint 运行时推迟到 change 02**：design.md D3 / tasks.md 2.2 原写 slint feature `backend-esp-idf`，但 slint 1.6/1.17 均无此 cargo feature。实际 slint 在 ESP-IDF 上需要：(a) `renderer-software` + `libm`（已用），(b) 自定义 `slint::platform::Platform` 后端驱动 ST7789（BSP 工作，change 02），(c) 修补 fontique/memmap2 在 espidf 上的可移植性缺陷（`libc` crate 不导出 `PROT_*/MAP_*/MS_*/MADV_*` 与 `mprotect/madvise/msync`；fontique 假设 64-bit 原子，RV32IMAC 无）。故 change 01 不将 slint 列为运行时依赖，`slint-build` 仍为 build-dep 以在编译期校验 `ui/boot.slint`。`main.rs` 暂以 `info!` + `FreeRtos::delay_ms` 阻塞循环替代 `BootWindow::show().run()`，等 change 02 接入平台后端后回切。
- **6.2 VerticalBox 导入**：slint 1.17 要求 `import { VerticalBox } from "std-widgets.slint";`，原 boot.slint 缺此导入。
- **构建基建**：`.cargo/config.toml [env]` 新增 `IDF_PATH`（指向已安装的本地 ESP-IDF 树，使 esp-idf-sys 走 `EspIdfOrigin::Custom` 而非 `git remote show origin` 联网验证——受限网络下 github.com 不可达）与 `ESP_IDF_GLOB_PARTITIONS_BASE`/`_CSV`（把 `partitions.csv` 复制进 esp-idf-sys CMake out 目录，因 ESP-IDF 以 cmake `PROJECT_DIR`=out 目录为基准解析 `CONFIG_PARTITION_TABLE_CUSTOM_FILENAME`）。两者均用 `relative = true` 保持可移植。

## 9. 烧录验收（硬件任务，需物理设备）

- [ ] 9.1 连接 Waveshare ESP32-C6-Touch-LCD-1.54 到 USB，确认 `/dev/tty.usb*` 或 `/dev/ttyUSB*` 出现
- [ ] 9.2 执行 `cargo espflash flash --port <port> --release`
- [ ] 9.3 上电后确认：LCD 背光点亮、屏幕显示 "Intercom Boot OK" 文字、文字清晰可见、画面持续显示
- [ ] 9.4 通过串口监视器（115200 8N1）确认启动日志包含 "Intercom boot OK"

> 9.1–9.4 需物理 Waveshare ESP32-C6-Touch-LCD-1.54 设备与 USB 连接，本次会话无硬件，留待用户在设备上执行。注意：9.3 的 LCD 文字渲染依赖 change 02 接入 slint 平台后端 + ST7789 驱动后方可生效；change 01 烧录后仅可通过串口日志（9.4）验证启动。
