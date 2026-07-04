## Why

17 个变更提案的第 16 个。前期变更（03 storage-service、07 settings-app）已建立 NVS 读写骨架与 Settings 页面框架，但缺少 schema_ver 兼容性校验、异常重启诊断、安全启动回退、恢复出厂等安全网机制。若数据损坏或固件升级后 NVS schema 不匹配，设备将卡死或加载非法状态。本期补齐"启动自检 → 数据校验 → 安全回退 → 诊断展示 → 恢复出厂"完整闭环，确保设备在任何数据异常下不卡死、可恢复。

## What Changes

- 实现 schema_ver 兼容性规则（§7.5）：NVS schema_ver == 固件 ver → 正常加载；NVS < 固件 → 系统设置回退默认 + 组信息清理；NVS > 固件 → 视为不兼容，组信息清理；读取失败/字段非法 → 同上
- 实现 abnormal_boot_cnt 递增逻辑：启动早期读取 PowerService::reset_reason()，仅异常重启（!= PowerOn）时递增 diag.abnormal_boot_cnt；同时将 reset_reason() 写入 diag.last_reset_reason（反映上次启动的复位原因）；连续超阈值（BoardProfile::ABNORMAL_BOOT_THRESHOLD=3）→ 进入安全启动模式
- 实现安全启动模式：仅 Settings 可用，不恢复对讲服务，允许进入 Settings 清空组信息或恢复出厂；safe_boot_flag 持久化到 diag 命名空间
- 新增 Settings > 关于页面：展示固件版本、上次复位原因（来自 DiagInfo.last_reset_reason，即上次启动的复位原因，映射为 PowerOn/Brownout/Wdt/Panic/Unknown）、连续异常重启次数、是否进入过安全启动
- 实现恢复出厂：清空 sys + group + diag 全部 NVS 命名空间 + 删除本机私钥/对端公钥总表 → 回到未组网 → 重启按首次使用路径启动
- 实现数据损坏无死锁保障：系统设置损坏 → 回退默认；组信息损坏 → 清理；任何路径不得卡死或阻塞启动
- 组队时 schema_ver 不一致：Host 在 PAIR_JOIN_REQ 校验阶段拒绝，返回 PAIR_JOIN_ACK accepted=0 reason=2（SchemaIncompatible）

## Capabilities

### New Capabilities
- `safety-diagnostics`: 启动安全自检、schema_ver 兼容性校验、异常重启计数与安全启动模式、诊断信息展示、恢复出厂、数据损坏无死锁保障

### Modified Capabilities
<!-- 本期为首次引入 safety-diagnostics，无既有 spec 修改 -->

## Impact

- **代码**：`src/services/storage_service.rs`（schema_ver 校验逻辑、diag 读写、clear_diag）、`src/services/power_service.rs`（reset_reason / abnormal_boot_count 已有 trait，补实现）、`src/apps/settings/`（About 页面 + 恢复出厂 UI）、`src/main.rs`（启动流程插入 abnormal_boot 检查 + schema_ver 校验 + 安全启动分支）、`src/intercom/`（组队 PAIR_JOIN_REQ 的 schema_ver 校验钩子）
- **NVS**：`diag` 命名空间实际写入（abnormal_boot_cnt / safe_boot_flag / last_reset_reason）；恢复出厂清空全部 3 个命名空间
- **依赖**：依赖 change 03 的 StorageService trait（load_diag / inc_abnormal_boot / clear_diag / schema_ver 字段）、change 04 的 PowerService（reset_reason / ResetReason 枚举）、change 06 的 NetworkService（组队阶段诊断信息）、change 07 的 Settings App 框架（新增 About 子页面 + 恢复出厂入口 + 安全启动模式守卫读取 safe_boot_flag）、change 09 的组队协议（PAIR_JOIN_REQ/ACK schema_ver 跨设备校验钩子）
- **后续变更**：change 17（integration-polish）在此基础上做全链路集成验证；OTA 预留分区已在 change 01 落地，本期不实现 OTA 下载
- **BoardProfile**：新增 `FIRMWARE_VERSION`、`SCHEMA_VER`、`ABNORMAL_BOOT_THRESHOLD` 常量
