## ADDED Requirements

### Requirement: schema_ver 兼容性校验
系统在启动时 SHALL 对 `sys` 与 `group` 两个 NVS 命名空间的 `schema_ver` 字段执行兼容性校验。当 NVS schema_ver 等于固件当前 SCHEMA_VER 时 SHALL 正常加载。当 NVS schema_ver 小于固件 ver 时 SHALL 将系统设置回退为默认值并清理组信息。当 NVS schema_ver 大于固件 ver 时 SHALL 视为不兼容并清理组信息。当 NVS 读取失败或字段非法时 SHALL 执行与上述相同的回退/清理逻辑。固件当前 SCHEMA_VER MUST 作为编译期常量定义在 BoardProfile 中。

#### Scenario: schema_ver 匹配正常加载
- **WHEN** 启动时读取 NVS sys.schema_ver == BoardProfile::SCHEMA_VER 且 group.schema_ver == BoardProfile::SCHEMA_VER
- **THEN** 系统设置与组信息均按 NVS 存储内容正常加载，启动流程继续

#### Scenario: NVS schema_ver 低于固件 ver
- **WHEN** 启动时读取 NVS sys.schema_ver < BoardProfile::SCHEMA_VER
- **THEN** 系统设置回退为默认值（调用 reset_settings），组信息被清理（调用 clear_group），启动流程继续不卡死

#### Scenario: NVS schema_ver 高于固件 ver
- **WHEN** 启动时读取 NVS group.schema_ver > BoardProfile::SCHEMA_VER
- **THEN** 组信息被视为不兼容并被清理（调用 clear_group），启动流程继续不卡死

#### Scenario: NVS 读取失败或字段非法
- **WHEN** StorageService::load_settings() 返回 Err(Io) 或 Err(Corrupt) 或解析出的 schema_ver 字段类型不匹配
- **THEN** 系统设置回退为默认值，启动流程继续不卡死

#### Scenario: group NVS 读取失败
- **WHEN** StorageService::load_group() 返回 Err(Io) 或 Err(Corrupt) 或 Err(SchemaMismatch)
- **THEN** 组信息被清理（调用 clear_group），设备进入未组网状态，启动流程继续不卡死

### Requirement: abnormal_boot_cnt 递增与安全启动模式
系统 SHALL 在 main 函数入口最早阶段（日志初始化之后、BSP 初始化之前）读取 PowerService::reset_reason()。仅当 reset_reason != PowerOn（即异常重启：Brownout/Wdt/Panic/Unknown）时 SHALL 调用 StorageService::inc_abnormal_boot() 递增 diag.abnormal_boot_cnt。正常上电（PowerOn）SHALL NOT 递增计数。同时 SHALL 将 PowerService::reset_reason() 返回值写入 diag.last_reset_reason（反映上次启动的复位原因）。当 abnormal_boot_cnt 大于等于 BoardProfile::ABNORMAL_BOOT_THRESHOLD（值为 3）时 SHALL 进入安全启动模式。安全启动模式 SHALL 仅启动 DisplayService 与 Settings App，SHALL NOT 启动 IntercomService 或 NetworkService。进入安全启动模式时 SHALL 持久化 diag.safe_boot_flag = true。

#### Scenario: 正常上电不递增计数
- **WHEN** 设备正常上电（PowerService::reset_reason() == PowerOn）启动且完整走完启动流程（schema_ver 校验通过 + 组信息加载或确认未组网 + 进入前台）
- **THEN** diag.abnormal_boot_cnt 不被递增，diag.abnormal_boot_cnt 被清零，diag.safe_boot_flag 被清除

#### Scenario: 连续异常重启进入安全启动
- **WHEN** 设备连续 3 次异常重启（abnormal_boot_cnt 累计到 3）
- **THEN** 设备进入安全启动模式，LCD 显示 Settings 页面，对讲服务不启动，diag.safe_boot_flag 被写入 true

#### Scenario: 安全启动模式下用户清空组信息
- **WHEN** 用户在安全启动模式的 Settings 中选择清空组信息并重启
- **THEN** clear_group() 被调用，设备软重启后 abnormal_boot_cnt 归零，走正常启动流程

#### Scenario: inc_abnormal_boot 本身失败
- **WHEN** StorageService::inc_abnormal_boot() 返回 Err（NVS 损坏）
- **THEN** 系统不阻塞启动，使用默认值 0 继续启动流程

### Requirement: 安全启动 flag 持久化
diag.safe_boot_flag SHALL 在进入安全启动模式时被写入 NVS 并持久化。该 flag SHALL 仅在恢复出厂（clear_diag）时被清除。用户在安全启动模式中清空组信息或恢复出厂后重启时，若 abnormal_boot_cnt 已归零，设备 SHALL 退出安全启动模式。

#### Scenario: safe_boot_flag 跨重启持久
- **WHEN** 设备进入安全启动模式后用户直接断电重启（未执行任何操作）
- **THEN** 重启后 safe_boot_flag 仍为 true，但是否进入安全启动模式取决于当前 abnormal_boot_cnt 是否超阈值

#### Scenario: 恢复出厂清除 safe_boot_flag
- **WHEN** 用户执行恢复出厂操作
- **THEN** clear_diag() 清空 diag 命名空间，safe_boot_flag 与 abnormal_boot_cnt 均归零

### Requirement: Settings 关于页面
Settings App SHALL 包含"关于"子页面，展示以下只读诊断信息：固件版本（来自 BoardProfile::FIRMWARE_VERSION 编译期常量）、上次复位原因（来自 DiagInfo.last_reset_reason，即启动早期写入 NVS 的 PowerService::reset_reason() 值，反映上次启动的复位原因，映射为 PowerOn/Brownout/Wdt/Panic/Unknown）、连续异常重启次数（来自 diag.abnormal_boot_cnt）、是否进入过安全启动（来自 diag.safe_boot_flag）。关于页面 SHALL 为只读展示，不提供任何编辑交互。

#### Scenario: 关于页面展示诊断信息
- **WHEN** 用户在 Settings 中进入关于页面
- **THEN** 页面显示固件版本字符串、复位原因文本、异常重启次数数值、安全启动标记布尔状态

#### Scenario: 正常启动后查看关于页面
- **WHEN** 设备正常启动后用户进入关于页面
- **THEN** 异常重启次数显示为 0，安全启动标记显示为否

#### Scenario: 安全启动后查看关于页面
- **WHEN** 设备从安全启动模式进入 Settings 关于页面
- **THEN** 异常重启次数显示为 ≥3，安全启动标记显示为是

### Requirement: 恢复出厂
Settings App SHALL 提供"恢复出厂"入口，触发后 SHALL 依次执行：reset_settings()（清空 sys 命名空间）、clear_group()（清空 group 命名空间，含本机私钥与对端公钥总表）、clear_diag()（清空 diag 命名空间）。全部清空完成后 SHALL 执行软重启。重启后设备 SHALL 走首次使用路径（NVS 全空 → 未组网 → Launcher）。

#### Scenario: 恢复出厂完整流程
- **WHEN** 用户在 Settings 中选择恢复出厂并确认
- **THEN** sys、group、diag 三个 NVS 命名空间被清空，设备软重启，重启后进入未组网首次使用状态

#### Scenario: 恢复出厂后密钥被删除
- **WHEN** 恢复出厂完成后检查 group 命名空间
- **THEN** my_priv_key 与 peers blob 均不存在，设备无法恢复任何既有组关系

#### Scenario: 恢复出厂后诊断归零
- **WHEN** 恢复出厂完成后重启并查看关于页面
- **THEN** 异常重启次数为 0，安全启动标记为否，复位原因为 PowerOn

### Requirement: 数据损坏无死锁
系统在启动流程中遇到的任何 NVS 数据损坏（sys/group/diag 任一命名空间）SHALL NOT 导致设备卡死、无限重启循环或 panic。系统设置损坏 SHALL 回退默认值。组信息损坏 SHALL 被清理。诊断信息损坏 SHALL 使用默认值（zeros）。回退/清理操作本身若失败，系统 SHALL 记录日志并继续启动。

#### Scenario: sys 命名空间全部损坏
- **WHEN** NVS sys 命名空间数据完全损坏（CRC 失败或 blob 长度非法）
- **THEN** 系统设置回退为默认值，设备正常启动到 Launcher 或上次前台

#### Scenario: group 命名空间部分字段损坏
- **WHEN** NVS group 命名空间中 peers blob 长度不合法（非 38 的整数倍）
- **THEN** 整个组信息被清理，设备进入未组网状态

#### Scenario: diag 命名空间损坏
- **WHEN** NVS diag 命名空间读取失败
- **THEN** abnormal_boot_cnt 视为 0，safe_boot_flag 视为 false，last_reset_reason 视为 Unknown，启动流程继续

#### Scenario: 回退操作本身失败
- **WHEN** reset_settings() 或 clear_group() 调用时 NVS 写入失败
- **THEN** 系统记录 error 日志，继续启动流程，不 panic 不阻塞

### Requirement: 组队 schema_ver 跨设备校验
Host 在收到 PAIR_JOIN_REQ 时 SHALL 校验请求方携带的 schema_ver 与本机 BoardProfile::SCHEMA_VER 是否一致。不一致时 SHALL 返回 PAIR_JOIN_ACK accepted=0 reason=2（SchemaIncompatible）。一致且其他校验通过时 SHALL 返回 accepted=1（Accepted）。PAIR_JOIN_ACK reason 码定义：0=Accepted、1=Full（满员）、2=SchemaIncompatible（不兼容）、3=StateChanged（状态变化）。

#### Scenario: 双方 schema_ver 一致
- **WHEN** Host 收到 PAIR_JOIN_REQ 且请求方 schema_ver == BoardProfile::SCHEMA_VER
- **THEN** Host 返回 PAIR_JOIN_ACK accepted=1，组队流程继续

#### Scenario: 双方 schema_ver 不一致
- **WHEN** Host 收到 PAIR_JOIN_REQ 且请求方 schema_ver != BoardProfile::SCHEMA_VER
- **THEN** Host 返回 PAIR_JOIN_ACK accepted=0 reason=2（SchemaIncompatible），请求方收到后返回未组网并显示失败原因

#### Scenario: Join 侧收到不兼容拒绝
- **WHEN** Join 收到 PAIR_JOIN_ACK accepted=0 reason=2（SchemaIncompatible）
- **THEN** Join 返回未组网状态，UI 显示"版本不兼容"失败提示
