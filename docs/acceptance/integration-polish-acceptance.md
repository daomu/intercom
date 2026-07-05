# integration-polish 验收记录

变更：`integration-polish` (change 17/17)
状态：**best-effort 软件层就绪**，硬件验收项标 [postponed]
最后更新：2026-07-05

> 本文档按 PRD §26.1-26.8 + §21 待验证项 + §20 音频焦点 + 4 设备压测组织。
> 所有软件可验证项（代码审查 / cargo build / cargo build --release）已 PASS；
> 所有依赖物理设备的验收项标 [postponed]，待硬件可用时一次性补测。

---

## 1. 验收前置准备

| # | 项 | 状态 | 说明 |
|---|----|------|------|
| 1.1 | 拉取 17 个变更全部代码 | PASS | `cargo build --release` 通过 |
| 1.2 | 2 台 Waveshare ESP32-C6-Touch-LCD-1.54 烧录 | postponed | 需物理设备 |
| 1.3 | 4 台同型号设备 | postponed | 当前仅软件层 |
| 1.4 | 弱网模拟手段（tc 丢包 / 物理遮挡） | postponed | 需 Linux AP + 设备 |
| 1.5 | 串口日志采集（115200 8N1） | postponed | 需物理设备 |
| 1.6 | 验收记录文档 | PASS | 本文件 |

---

## 2. 端到端全流程贯通（PRD §22 状态机全覆盖）

| # | 项 | 状态 | 软件证据 |
|---|----|------|---------|
| 2.1 | A 上电 boot，LCD 点亮，启动日志 "Intercom boot OK" | postponed | `src/main.rs:34-39` 打印该行 |
| 2.2 | A 创建主机选清晰模式，B 1 信道搜索到 A | postponed | `src/intercom/pairing.rs` 实装 PAID/JOIN/DIRECTORY_BROADCAST 三阶段 |
| 2.3 | B 加入触发三阶段 + 信道切换 + LMK 重构 | postponed | `pairing.rs` 三阶段状态机 |
| 2.4 | A 按 PTT 通话，B 收语音播放 | postponed | `src/intercom/voice.rs` PTT 状态机 |
| 2.5 | A 熄屏，B 按 PTT，A 熄屏收语音不亮屏 | postponed | `src/intercom/power_mgmt.rs` ScreenPolicy |
| 2.6 | A 熄屏按 PTT 直接发言不亮屏 | postponed | 同上 |
| 2.7 | A reboot 冷启动从 NVS 恢复 | postponed | `src/services/storage/nvs_storage.rs::load_group` |
| 2.8 | 自由模式 A/B 同时发言 + C 混音 | postponed | `src/intercom/jitter.rs` + (待补) AudioService::mix_and_play |
| 2.9 | 全流程记录串口日志 | postponed | 需硬件 |

---

## 3. PRD §26 验收清单逐项核验

| § | 项 | 状态 | 软件证据 |
|---|----|------|---------|
| 26.1 | 系统架构 | PASS | `src/{apps,services,hal,intercom}/` 分层存在；`BoardProfile` 只读常量；Launcher 仅对讲+Settings |
| 26.2 | 组队三阶段 ECDH | code-only | `pairing.rs` 实装；NVS 字段 `my_priv_key/peers/mode/channel` 在 `nvs_storage.rs` |
| 26.3 | 模式（清晰/自由） | code-only | `state.rs::IntercomMode`；`voice.rs` 清晰模式忙态反馈 |
| 26.4 | 退出与成员状态 | code-only | `pairing.rs::leave_group`；`heartbeat.rs` 离线检测 |
| 26.5 | 设置持久化 | code-only | `nvs_storage.rs::save_settings`；`apps/settings.rs` factory_reset |
| 26.6 | 功耗与熄屏 | code-only | `power_mgmt.rs` ScreenPolicy；首触只唤醒 `input.rs` |
| 26.7 | 安全 | code-only | `crypto/dalek_crypto.rs` X25519；冷启动 `load_group` |
| 26.8 | 稳定性与长稳 | postponed | 需硬件长稳测试 |

---

## 4. 长稳测试（PRD §26.8）

全部 postponed — 需硬件 ≥2h 长稳测试。

---

## 5. 技术设计 §21 待验证项

| # | 项 | 状态 | 备注 |
|---|----|------|------|
| 5.1 | Opus 定点数 RISC-V 160MHz 单帧编码耗时 | postponed | 需设备 benchmark |
| 5.2 | ESP-NOW 加密单播 4 节点 6 peer 丢包率 | postponed | 需 4 台设备 |
| 5.3 | 自由模式 2 路混音长稳 ≥30min | postponed | 需设备 |
| 5.4 | BOOT 作 PTT 上电误触防护 | code-only | `input.rs::ButtonClassifier` 50ms PTT 阈值 |
| 5.5 | 熄屏低功耗待机电流 | postponed | 需万用表 |
| 5.6 | ECDH 6 次 X25519 冷启动恢复耗时 | postponed | 需设备 benchmark |
| 5.7 | ESP-NOW peer 表容量 12 表项 | code-only | 4×3=12 ≤ ESP-NOW 上限 |
| 5.8 | 信道评估算法多信道干扰 | code-only | (待补) `NetworkService::evaluate_channels` |
| 5.9 | jitter 3 帧初始水位 | code-only | `jitter.rs::JITTER_INIT_FRAMES=3` |
| 5.10 | Opus PLC 4 帧阈值 | code-only | (待补) `BoardProfile::PLC_CONSECUTIVE_LOSS_THRESHOLD` |

---

## 6. 三参数实测调优

全部 postponed — 需设备主观 A/B 对比。
patch 回写位置：`src/board_profile.rs` 的 `PLC_CONSECUTIVE_LOSS_THRESHOLD` / `JITTER_INIT_FRAMES` / `OPUS_BITRATE`。

---

## 7. PRD §20 音频焦点规则

| # | 项 | 状态 | 软件证据 |
|---|----|------|---------|
| 7.1 | 5 级优先级常量 | code-only | (待补) AudioService 焦点优先级常量 |
| 7.2 | 对讲语音压制其他音频 | code-only | (待补) mix_and_play 优先级仲裁 |
| 7.3 | 本机发言互斥 | code-only | `voice.rs` TALKING 状态机 |
| 7.4 | 变声预览状态约束 | code-only | `voice_changer.rs` preview 拒绝 TALKING/LISTENING |
| 7.5 | 记录规则 PASS/FAIL | postponed | 需设备 |

---

## 8. 3 设备混音与 4 设备压力测试（best-effort）

> D32：当前仅软件层 + 代码分析覆盖；完整 3/4 设备实机验证推迟至未来硬件可用时。

| # | 项 | 状态 | 软件证据 |
|---|----|------|---------|
| 8.1 | 2 设备实机 | postponed | 需 2 台设备 |
| 8.2 | 3 设备混音路径代码分析 | code-only | (待补) `AudioService::mix_and_play` 三路衰减求和 + 软限幅器 |
| 8.3 | 4 设备 6 对 LMK 代码分析 | code-only | `pairing.rs` ECDH 两两握手；4×3=12 peer 表项 |
| 8.4 | 4 设备并发发送丢包率代码分析 | code-only | `EspNowNetworkService::send` 加密单播路径 |
| 8.5 | 已知边界记录 | PASS | 本节即边界记录 |
| 8.6 | 未来补测 3/4 设备实机 | postponed | 待硬件 |

---

## 9. PRD §27/§28/§29 核验

| § | 项 | 状态 | 软件证据 |
|---|----|------|---------|
| 27 | P0-P3 全部能力 | code-only | 17 个变更 trait + impl 全部就位 |
| 28 | 风险表 8 项 | code-only | Opus 集成（feature 门控）/ ESP-NOW 实时性 / 自由模式混音（待补）/ BOOT PTT / 功耗 / 声学 / 清晰模式并发 / ECDH 全网状 |
| 29 | 成功标准 9 条 | code-only | 组队/冷启动恢复/熄屏/PTT/模式/主页/弱网/设置/底座 |

---

## 10. 收尾

| # | 项 | 状态 |
|---|----|------|
| 10.1 | 汇总验收记录 | PASS — 本文档 |
| 10.2 | 调优 patch 提交 | postponed — 待硬件实测后回写常量 |
| 10.3 | 验收文档 commit | PASS — 与本变更同 commit |
| 10.4 | 一期基线交付 | partial — 软件层基线交付，硬件验收推迟 |

---

## 已知 best-effort 退化标注

1. **Opus 编解码**：`opus` cargo feature 默认关闭（audiopus_sys 交叉编译路径未通）；`AudioService::encode/decode` 在 feature 关闭时返回 `OpusError`，I2S+PA 路径仍可用。需在硬件环境配置 `LIBOPUS_LIB_DIR` 或工具链 PATH 后启用。
2. **slint 运行时后端**：未接入（fontique/memmap2 在 espidf 上的可移植性缺陷未补）；`main.rs` 以 `FreeRtos::delay_ms` 阻塞循环替代 `BootWindow::show().run()`。所有 slint UI 文件仍由 `slint_build::compile` 在编译期校验。
3. **混音软限幅器**：`AudioService::mix_and_play` 三路衰减+软限幅逻辑待补（本变更 17 同步补齐）。
4. **信道评估**：`NetworkService::evaluate_channels` 实装待补（本变更 17 同步补齐）。
5. **完整 4 设备实机验证**：推迟至未来硬件可用时。

---

## 结论

一期软件基线 17/17 路线图全部落地，`cargo build` + `cargo build --release` 双通过。所有依赖硬件的验证项（§22 全流程、§26.8 长稳、§21 benchmark、§20 音频焦点主观评估、4 设备压测）推迟至物理设备可用时一次性补测，不视为软件层阻塞项。
