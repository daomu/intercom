## MODIFIED Requirements

### Requirement: 亮度调节副作用
`SettingsApp` 亮度变更 SHALL 经 controller 调用 `HalDisplayService::set_brightness(u8)` 立即生效，SHALL NOT 仅写入 NVS 持久化。占空比下限 SHALL ≥ 5% 以避免完全黑屏使界面失去反馈。`set_brightness` SHALL 同时持久化到 NVS 与触发 backlight 硬件更新。

#### Scenario: 滑动亮度条即时生效
- **WHEN** 用户在亮度页拖动滑块从 80 到 30
- **THEN** `SettingsApp::set_brightness(30)` 返回 `BrightnessChanged(30)`，main loop 调 `display.set_brightness(30)`，屏幕亮度立即降低，且 30 被持久化到 NVS

#### Scenario: 亮度 0 不全黑
- **WHEN** 用户把亮度滑到 0
- **THEN** BacklightDriver SHALL clamp 到最小 5% 占空比，屏幕仍可见，NVS 存储 0

#### Scenario: 重启后恢复亮度
- **WHEN** 设备重启，Settings 从 NVS 读取 `brightness = 30`
- **THEN** `Hal::init()` 时 BacklightDriver 应用 30% 占空比，屏幕亮度与关机前一致

### Requirement: 恢复出厂二次确认
`SettingsApp::factory_reset_confirm()` 在二次确认后 SHALL 返回 `SettingsOutcome::FactoryResetRequested`。controller 接到该 outcome SHALL 调用 `StorageService::factory_reset()` 清空 NVS namespace 后 `esp_restart()`。`factory_reset()` 失败时 controller SHALL 仍 `esp_restart()` 但 log::error 记录失败原因，SHALL NOT 保留半状态继续运行。

#### Scenario: 二次确认触发真重置
- **WHEN** 用户在恢复出厂页第一次点击"确认"进入二次确认，再点击"恢复"
- **THEN** `factory_reset_confirm()` 返回 `true`，main loop 调 `storage.factory_reset()` 清 NVS 后 `esp_restart()`，设备重启后回到首启状态（无 group / 默认设置）

#### Scenario: NVS 清空失败仍重启
- **WHEN** `storage.factory_reset()` 返回 Err（NVS 损坏）
- **THEN** main loop log::error 记录失败，但仍 `esp_restart()`，避免保留半状态继续运行

### Requirement: 关于页真实诊断数据
`SettingsApp` 关于页 SHALL 显示真实诊断数据：`fw_version`（`env!("CARGO_PKG_VERSION")` 编译期注入）/ `last_reset_reason`（来自 `SafetyService`）/ `abnormal_restart_count`（来自 `SafetyService`）/ `secure_boot_enforced`（来自 `SafetyService`）。controller SHALL 每 tick 从 service 快照 `AboutData` 注入 `SettingsApp`，SHALL NOT 让 model 直接持有 service 引用。SHALL NOT 显示硬编码占位字符串。

#### Scenario: 关于页显示真实版本
- **WHEN** 用户从 Settings 主菜单进入"关于"页
- **THEN** 顶部显示 `fw_version` 来自 `env!("CARGO_PKG_VERSION")`（如 "0.1.0"），非硬编码 "vX.Y"

#### Scenario: 异常重启次数刷新
- **WHEN** 设备上次因 panic 重启，启动后用户进入关于页
- **THEN** `abnormal_restart_count` 显示 1（或递增后的值），数据来自 `SafetyService` 持久化的计数器
