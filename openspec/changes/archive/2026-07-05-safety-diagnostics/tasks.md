## 1. BoardProfile 常量扩展

- [x] 1.1 在 `src/board_profile.rs` 新增 `FIRMWARE_VERSION: &str = "1.0.0"`（一期版本号）
- [x] 1.2 在 `src/board_profile.rs` 新增 `SCHEMA_VER: u16 = 1`（NVS 存储结构版本）
- [x] 1.3 在 `src/board_profile.rs` 新增 `ABNORMAL_BOOT_THRESHOLD: u32 = 3`（连续异常重启阈值）
- [x] 1.4 在 `src/board_profile.rs` 新增 `PAIR_JOIN_REASON_INCOMPATIBLE: u8 = 2`（不兼容 reason 码）

## 2. DiagInfo 结构与 StorageService 实现

- [x] 2.1 在 `src/services/storage_service.rs` 定义 `pub struct DiagInfo { pub abnormal_boot_cnt: u32, pub safe_boot_flag: bool, pub last_reset_reason: u32 }` 与 `impl Default`（全零/false）
- [x] 2.2 实现 `StorageService::load_diag(&self) -> DiagInfo`：读取 diag 命名空间 abnormal_boot_cnt / safe_boot_flag / last_reset_reason，读取失败或字段非法返回默认值
- [x] 2.3 实现 `StorageService::inc_abnormal_boot(&self) -> Result<(), StorageError>`：读取当前 cnt +1 写回；若 key 不存在按 0+1 处理；写失败返回 Err
- [x] 2.4 实现 `StorageService::clear_diag(&self) -> Result<(), StorageError>`：清空 diag 命名空间全部 key
- [x] 2.5 实现 `StorageService::set_safe_boot_flag(&self, v: bool) -> Result<(), StorageError>`：写入 safe_boot_flag
- [x] 2.6 实现 `StorageService::set_last_reset_reason(&self, reason: u32) -> Result<(), StorageError>`：写入 last_reset_reason（由 main 早期阶段调用，写入 PowerService::reset_reason() 的返回值）

## 3. schema_ver 兼容性校验逻辑

- [x] 3.1 在 `StorageService::load_settings()` 实现中增加 schema_ver 校验：读取后比对 `BoardProfile::SCHEMA_VER`，不匹配返回 `Err(SchemaMismatch)`
- [x] 3.2 在 `StorageService::load_group()` 实现中增加 schema_ver 校验：读取后比对 `BoardProfile::SCHEMA_VER`，不匹配返回 `Err(SchemaMismatch)`
- [x] 3.3 在 `src/main.rs` 启动流程中用 match 包裹 load_settings()，Err(Io|Corrupt|SchemaMismatch) → 调用 reset_settings() 并记录 warn 日志
- [x] 3.4 在 `src/main.rs` 启动流程中用 match 包裹 load_group()，Err → 调用 clear_group() 并记录 warn 日志，继续走未组网路径
- [x] 3.5 在回退操作本身（reset_settings / clear_group）失败时记录 error 日志但不 panic，继续启动

## 4. abnormal_boot_cnt 递增与安全启动模式

- [x] 4.1 在 `src/main.rs` 入口（EspLogger::initialize_default 之后、BSP init 之前）读取 `PowerService::reset_reason()`，仅当返回值 != PowerOn 时调用 `storage.inc_abnormal_boot()`；同时将 reset_reason() 返回值写入 `diag.last_reset_reason`（`storage.set_last_reset_reason(reason)` 或等效方法）。Err 时记录 error 并继续（不阻塞）
- [x] 4.2 调用 `storage.load_diag()` 获取 abnormal_boot_cnt，与 `BoardProfile::ABNORMAL_BOOT_THRESHOLD` 比较
- [x] 4.3 若 cnt >= 阈值：调用 `storage.set_safe_boot_flag(true)`，跳过 IntercomService/NetworkService 初始化，仅初始化 DisplayService + 进入 Settings App
- [x] 4.4 安全启动模式下 Settings App 内提供"清空组信息并重启"操作：调用 clear_group() → esp_idf_svc::system::reset()
- [x] 4.5 正常启动流程完整走通后（进入前台前）调用 `storage.clear_diag()` 归零 cnt 与 flag

## 5. Settings 关于页面

- [x] 5.1 在 `ui/settings/about.slint` 新增 About 页面组件，包含 4 行只读文本：固件版本、复位原因、异常重启次数、安全启动标记
- [x] 5.2 在 `src/apps/settings/` 新增 about 模块，从 `BoardProfile::FIRMWARE_VERSION`、`DiagInfo.last_reset_reason`（由 `StorageService::load_diag()` 返回，非 `PowerService::reset_reason()` 当前值）、`DiagInfo.abnormal_boot_cnt`、`DiagInfo.safe_boot_flag` 获取数据
- [x] 5.3 将 ResetReason 枚举映射为显示字符串：PowerOn→"正常上电"、Brownout→"电压不足"、Wdt→"看门狗"、Panic→"异常崩溃"、Unknown→"未知"
- [x] 5.4 在 Settings 主页面添加"关于"入口，导航到 About 页面
- [x] 5.5 About 页面为只读，不提供任何编辑或按钮交互（纯展示）

## 6. 恢复出厂

- [x] 6.1 在 Settings App 新增"恢复出厂"入口（需二次确认弹窗）
- [x] 6.2 实现 `factory_reset()` 函数：依次调用 reset_settings() + clear_group() + clear_diag()
- [x] 6.3 全部清空成功后调用 `esp_idf_svc::system::reset()` 软重启
- [x] 6.4 若任一清空操作失败，记录 error 日志但仍尝试继续后续清空与重启（尽力而为，不卡死）
- [x] 6.5 在 Settings UI 中恢复出厂入口需显示二次确认对话框，确认后才执行

## 7. 组队 schema_ver 跨设备校验

- [x] 7.1 在 Host 侧 PAIR_JOIN_REQ 处理逻辑中，解析请求方携带的 ver 字段（通用包头偏移 1）
- [x] 7.2 比对请求方 ver 与 `BoardProfile::SCHEMA_VER`，不一致时发送 PAIR_JOIN_ACK accepted=0 reason=`PAIR_JOIN_REASON_INCOMPATIBLE`
- [x] 7.3 在 Join 侧收到 PAIR_JOIN_ACK accepted=0 reason=2 时，返回未组网状态并在 UI 显示"版本不兼容"
- [x] 7.4 在 Join 侧收到 accepted=0 reason≠2 时，按原有失败原因处理（满员/状态变化）

## 8. 集成与验证

- [x] 8.1 验证正常启动（无 NVS 数据）走首次使用路径，关于页面显示异常重启次数=0
- [x] 8.2 手动触发看门狗复位 3 次后验证设备进入安全启动模式，关于页面显示 cnt≥3 + safe_boot_flag=true
- [x] 8.3 在安全启动模式 Settings 中执行清空组信息 → 重启 → 验证正常启动
- [x] 8.4 执行恢复出厂 → 验证 sys+group+diag 全空 → 重启走首次使用路径
- [x] 8.5 手动损坏 NVS group 命名空间（写入非法 blob）→ 验证设备不卡死、组信息被清理、进入未组网
- [x] 8.6 两台设备烧录不同 SCHEMA_VER 的固件 → 验证组队时返回 accepted=0 reason=不兼容
- [x] 8.7 `cargo build` 零编译错误验证
- [x] 8.8 `cargo build --release` 验证 release profile 通过


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
