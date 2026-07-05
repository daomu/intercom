## Context

change 03 已定义 `StorageService` trait（含 `load_diag` / `inc_abnormal_boot` / `clear_diag`）、`Settings`/`GroupInfo` 结构（含 `schema_ver: u16` 字段）、`StorageError` 枚举（Io/SchemaMismatch/Corrupt）、NVS 三命名空间划分（sys/group/diag）。change 07 已建立 Settings App 框架（设备名称/音量/亮度等子页面）。技术设计 §7.4-7.5 定义了 diag 键值与 schema_ver 兼容性规则表，§8 启动流程含 abnormal_boot 检查分支，§17 异常处理表覆盖 4 类回退场景。PRD §5.3/§5.4/§6.4/§25.2/§26.5 定义安全启动回退、schema version、恢复出厂、最小诊断信息与设置验收标准。

当前缺口：schema_ver 校验逻辑未实现（NVS 可被旧固件写入后新固件读到不匹配 ver）；abnormal_boot_cnt 无递增逻辑；安全启动模式不存在；Settings 无关于页面与恢复出厂；组队时无 schema_ver 跨设备校验。

## Goals / Non-Goals

**Goals:**
- schema_ver 兼容性规则按 §7.5 表完整实现，覆盖 sys 与 group 两个命名空间
- abnormal_boot_cnt 在 bootloader 钩子阶段递增，超阈值进入安全启动模式
- 安全启动模式下仅 Settings 可用（对讲服务不启动），用户可清空组信息或恢复出厂
- Settings > 关于页面展示 4 项诊断信息（固件版本/复位原因/异常重启次数/安全启动标记）
- 恢复出厂清空 sys+group+diag 全部 NVS + 删除密钥 → 重启走首次使用路径
- 数据损坏（sys/group/diag 任意命名空间）不卡死、不阻塞启动
- 组队 PAIR_JOIN_REQ 阶段校验双方 schema_ver，不一致返回 accepted=0 reason=不兼容

**Non-Goals:**
- OTA 实际下载与双区切换（分区已预留，本期不实现 OTA 流程）
- 远程诊断上报或日志导出（仅本地 Settings 展示）
- 自动崩溃日志捕获与存储（仅计数与复位原因码）
- 安全启动模式下的固件修复/降级工具
- 空中协议版本字段（PRD §5.4 明确不保留，兼容性由 schema_ver 隐式承担）

## Decisions

### D1：schema_ver 当前版本 = 编译期常量
`BoardProfile::SCHEMA_VER: u16 = 1`（一期版本）。sys 与 group 共享同一 schema_ver 语义（技术设计 §7.3 "与包 ver 同步"）。固件升级时若 NVS 结构变化，递增此常量即可触发旧数据回退/清理。备选：运行时从构建脚本注入——不必要，编译期常量足够且更安全。

### D2：abnormal_boot_cnt 递增时机 = main 入口最早（仅异常重启）
在 `EspLogger::init` 之后、BSP init 之前，先读取 `PowerService::reset_reason()`，仅当返回值 != `PowerOn` 时才调用 `StorageService::inc_abnormal_boot()`。正常上电（PowerOn）不递增计数。理由：BSP init 可能 panic（如 I2C 设备未响应），需在此之前已写入计数；仅异常重启（Brownout/Wdt/Panic/Unknown）才应累计。同时将 `PowerService::reset_reason()` 返回值写入 `diag.last_reset_reason`（见 D8）。若 inc 本身失败（NVS 损坏），不阻塞启动，继续走默认值 0。阈值 `ABNORMAL_BOOT_THRESHOLD = 3`（连续 3 次异常重启进入安全启动）。备选：bootloader C 钩子——esp-idf bootloader 可定制但 Rust 侧无直接入口，且增加构建复杂度；改为 Rust main 最早行实现，等价语义且可维护。

### D3：安全启动模式 = 运行时分支 + safe_boot_flag 持久化
进入安全启动模式时写入 `diag.safe_boot_flag = true` 并持久化。此 flag 仅在恢复出厂时清零（用户主动操作）。安全启动模式下 main 流程跳过 IntercomService/NetworkService 初始化，仅启动 DisplayService + Settings App。`safe_boot_flag` 由 Launcher（change 07）读取并守卫：Launcher 检测到 `safe_boot_flag == true` 时 SHALL 仅显示 Settings App、SHALL 跳过 IntercomService/NetworkService 初始化、SHALL 显示"安全模式"指示器（详见 change 07 spec "安全启动模式守卫"）。本期（change 16）仅负责设置与持久化该 flag，实际守卫逻辑在 change 07 Launcher 中实现。用户在 Settings 内可：① 清空组信息（调用 `clear_group`）后软重启 → 重新走正常启动；② 恢复出厂 → 清空全部 NVS → 重启。备选：自动清除 safe_boot_flag——不安全，可能掩盖持续异常的根因。

### D4：成功启动后 abnormal_boot_cnt 清零策略
正常完成启动流程（通过 schema_ver 校验 + 组信息加载/未组网确认 + 进入正常前台）后，调用 `clear_diag()` 将 abnormal_boot_cnt 归零并清除 safe_boot_flag。理由：仅在"完整走通一次正常启动"后归零，确保半途崩溃仍累计。备选：仅清零 cnt 保留 flag——flag 语义为"曾进入过安全启动"，但恢复出厂已是更强的重置，不冲突。

### D5：恢复出厂 = 清空全部 3 命名空间 + 软重启
调用 `StorageService::reset_settings()`（清 sys）+ `clear_group()`（清 group，含密钥）+ `clear_diag()`（清 diag）后执行 `esp_idf_svc::system::reset()` 软重启。重启后 NVS 全空，走首次使用路径（未组网 → Launcher）。私钥/公钥总表存储在 group 命名空间的 my_priv_key 与 peers blob 中，clear_group 已覆盖删除。备选：仅清 sys+group 保留 diag——违反 PRD §6.4"清空"语义，且诊断计数应一并归零。

### D6：组队 schema_ver 校验 = PAIR_JOIN_REQ 处理侧
Host 收到 PAIR_JOIN_REQ 后，在 `validate_pubkey` 之后增加 `join_schema_ver == host_schema_ver` 校验。不一致时发送 `PAIR_JOIN_ACK accepted=0 reason=不兼容`（reason 码 = 2，对应技术设计 §5.4 reason 字段"满员/不兼容/状态变化"中的不兼容）。schema_ver 在 PAIR_JOIN_REQ 包中复用通用包头 `ver` 字段（§4.1 偏移 1，1 字节——但 schema_ver 为 u16，取低 8 位传输，一期 ver=1 无截断问题；若未来 ver>255 需扩展包头）。备选：新增独立 schema_ver 字段——增加包长度，一期无必要。

### D7：数据损坏无死锁 = try-load + match 回退
所有 NVS 读取用 `match` 包裹，`Err(StorageError::Io|Corrupt|SchemaMismatch)` 对 sys 走 `reset_settings()`、对 group 走 `clear_group()`、对 diag 走默认值（zeros）。任何回退操作本身若再失败，记录日志并继续启动（不 panic、不 loop）。备选：panic 并 reboot——可能导致 boot loop，违反"不得卡死"要求。

### D8：关于页面数据来源 = DiagInfo 结构（复位原因 = 上次启动）
`StorageService::load_diag()` 返回 `DiagInfo { abnormal_boot_cnt, safe_boot_flag, last_reset_reason }`。固件版本来自 `BoardProfile::FIRMWARE_VERSION`（编译期常量，格式 `"1.0.0"` 或 git-desert 注入）。复位原因来自 `DiagInfo.last_reset_reason`（而非 `PowerService::reset_reason()` 当前启动值）。在启动最早阶段（inc_abnormal_boot 之前或同时），将 `PowerService::reset_reason()` 返回值写入 `diag.last_reset_reason`，此时读到的 reset_reason 反映的是"导致本次重启的原因"即"上次启动的结局"。关于页面读取 `DiagInfo.last_reset_reason` 展示，映射 `ResetReason` 枚举为中文/英文显示字符串。Settings About 页面为只读展示，无编辑交互。

## Risks / Trade-offs

- **[abnormal_boot_cnt 递增在 Rust main 而非 C bootloader]** → 若 BSP init 之前 panic 发生在 Rust 运行时初始化阶段（如 .bss 清零失败），计数可能未递增。可接受：此类底层 panic 通常为硬件故障，软件层无法兜底。
- **[schema_ver 取低 8 位传输]** → 一期 ver=1 无问题；ver>255 时需扩展包头或分两字节。记录为已知技术债，change 17 集成评审时确认。
- **[安全启动模式下 DisplayService 仍需初始化]** → 若显示驱动本身损坏导致 panic，安全启动模式无法展示。可接受：显示硬件故障超出软件安全网范围，需硬件维修。
- **[clear_diag 与 inc_abnormal_boot 竞态]** → 单核单线程启动流程，无并发风险。
- **[恢复出厂后软重启可能因 NVS commit 延迟]** → esp-idf NVS commit 为同步写 Flash，调用返回即持久化；reset 前确保 commit 完成。

## Migration Plan

无既有运行时需迁移（本期为首次实现）。部署步骤：
1. 拉取本变更 commit
2. `cargo build` 验证编译
3. 烧录到设备，首次上电 NVS 全空 → 走首次使用路径
4. 手动触发异常重启（如看门狗超时）验证 abnormal_boot_cnt 递增
5. 连续 3 次异常重启验证安全启动模式
6. 在安全启动模式 Settings 内验证恢复出厂

回滚：`git revert <commit>`，回到无安全诊断逻辑的状态（NVS 损坏可能卡死，但基础功能不受影响）。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐。

## Tech Debt

### TD1：schema_ver 截断风险（D16）
通用包头 `ver` 字段为 1 字节（§4.1 偏移 1），而 `BoardProfile::SCHEMA_VER` 类型为 `u16`。当前一期 ver=1，取低 8 位传输无截断问题。若未来 schema_ver 超过 255，包头 `ver` 字段无法承载完整值，必须新增独立的 `ver_negotiation` 包类型（在 PAIR_JOIN_REQ 前交换完整 u16 schema_ver）。此为已知技术债，change 17 集成评审时确认。
