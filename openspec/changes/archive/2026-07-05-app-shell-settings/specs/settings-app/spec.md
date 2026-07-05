## ADDED Requirements

### Requirement: Settings App 页面集合
Settings App SHALL 在 `src/apps/settings_app.rs` 实现以下 slint 页面（`ui/settings.slint`）：设备名称页、系统音量页、全局静音页、亮度页、自动熄屏时间页、关于页、恢复出厂页。页面导航 SHALL 采用左右滑页切换（PRD §8.1 层级浅），不使用深层菜单。

#### Scenario: Settings 七页可切换
- **WHEN** 用户在 Settings App 内左右滑动
- **THEN** 按顺序切换：设备名称 → 系统音量 → 全局静音 → 亮度 → 自动熄屏时间 → 关于 → 恢复出厂

#### Scenario: 每页显示当前值
- **WHEN** 用户进入任一设置页
- **THEN** 该页显示从 `StorageService::load_settings` 读入的当前值（如音量页显示当前 `Settings.volume`）

### Requirement: 设备名称页（含随机生成）
设备名称页 SHALL 显示当前 `Settings.device_name`，提供编辑输入与"随机生成"按钮。随机生成 SHALL 从内置形容词+名词词表组合生成（如 "SwiftFox"），生成后 SHALL 调用 `StorageService::save_settings` 持久化。设备名称长度 SHALL 限制为 1-16 字符。

#### Scenario: 随机生成名称并持久化
- **WHEN** 用户点击"随机生成"按钮
- **THEN** 生成一个形容词+名词组合名称，显示在输入框，`Settings.device_name` 更新并 `save_settings` 持久化

#### Scenario: 手动编辑名称
- **WHEN** 用户在输入框编辑名称为 "MyIntercom" 并确认
- **THEN** `Settings.device_name = "MyIntercom"`，`save_settings` 被调用

#### Scenario: 空名称拒绝
- **WHEN** 用户清空输入框并确认
- **THEN** 名称不被接受，显示提示"名称不能为空"，`Settings.device_name` 保持原值

### Requirement: 系统音量页
系统音量页 SHALL 提供滑块（0-100），绑定 `Settings.volume`。调整 SHALL 即时调用 `StorageService::save_settings` 持久化。

#### Scenario: 调整音量即时持久化
- **WHEN** 用户拖动音量滑块从 50 到 70
- **THEN** `Settings.volume = 70`，`save_settings` 被调用

### Requirement: 全局静音开关页
全局静音页 SHALL 提供开关控件，绑定 `Settings.muted`。切换 SHALL 即时调用 `StorageService::save_settings` 持久化。PRD §6.3 静音影响范围 SHALL 包含对讲接收语音播放（change 05 AudioService 接入后生效，本期仅持久化状态）。

#### Scenario: 开启静音
- **WHEN** 用户将静音开关从 OFF 切换到 ON
- **THEN** `Settings.muted = true`，`save_settings` 被调用，状态栏静音图标刷新

### Requirement: 亮度页
亮度页 SHALL 提供滑块（0-100），绑定 `Settings.brightness`。调整 SHALL 即时调用 `DisplayService::set_brightness` 生效 AND `StorageService::save_settings` 持久化。

#### Scenario: 调整亮度即时生效
- **WHEN** 用户拖动亮度滑块从 80 到 50
- **THEN** `DisplayService::set_brightness(50)` 被调用，LCD 背光变暗，`Settings.brightness = 50`，`save_settings` 被调用

### Requirement: 自动熄屏时间页
自动熄屏时间页 SHALL 提供选项列表：5 秒 / 15 秒 / 30 秒 / 60 秒 / 常亮。选择 SHALL 即时更新 `Settings.screen_off_sec` 并调用 `StorageService::save_settings` 持久化。"常亮"选项 SHALL 将 `screen_off_sec` 设为一个特殊值（如 `u32::MAX`）表示永不熄屏。

#### Scenario: 选择 30 秒熄屏
- **WHEN** 用户选择 "30 秒" 选项
- **THEN** `Settings.screen_off_sec = 30`，`save_settings` 被调用，Launcher 熄屏计时器按新值运行

#### Scenario: 选择常亮
- **WHEN** 用户选择 "常亮" 选项
- **THEN** `Settings.screen_off_sec = u32::MAX`，Launcher 熄屏计时器永不触发 `screen_off`

### Requirement: 关于页（最小诊断信息）
关于页 SHALL 显示 PRD §25.2 规定的最小诊断四项：固件版本（编译期常量，从 `env!("CARGO_PKG_VERSION")` 或 build.rs 注入）、上次复位原因（`PowerService::reset_reason` 映射为可读文本）、异常重启次数（`PowerService::abnormal_boot_count`）、安全启动标记（从 `StorageService::load_diag` 读 `safe_boot_flag`）。

#### Scenario: 关于页显示四项诊断
- **WHEN** 用户进入关于页
- **THEN** 屏幕显示固件版本 / 上次复位原因 / 异常重启次数 / 安全启动标记四行信息

#### Scenario: 复位原因可读
- **WHEN** `PowerService::reset_reason()` 返回 `ResetReason::Panic`
- **THEN** 关于页显示"上次复位原因：Panic"或等价可读文本

### Requirement: 恢复出厂设置（两步确认）
恢复出厂页 SHALL 实现两步确认流程：第一屏显示警告文本与"确认"按钮；点击"确认"后进入第二屏显示"再次确认"与"取消"/"确认恢复"按钮。点击"确认恢复"后 SHALL 依次调用 `StorageService::reset_settings` / `clear_group` / `clear_diag`，然后触发软重启（`esp_restart`）。PRD §6.4 要求清空系统设置 + 组信息 + 私钥/公钥总表。

#### Scenario: 两步确认完整流程
- **WHEN** 用户在恢复出厂页点击"确认" → 再点击"确认恢复"
- **THEN** `reset_settings` / `clear_group` / `clear_diag` 依次被调用，设备软重启，重启后 NVS 全空走首次使用路径

#### Scenario: 取消恢复出厂
- **WHEN** 用户在第二屏点击"取消"
- **THEN** 返回 Settings 首页或恢复出厂第一屏，NVS 未被清理

#### Scenario: 一步直接点击不触发
- **WHEN** 用户在第一屏点击"确认"后直接断电（未点击第二屏"确认恢复"）
- **THEN** NVS 未被清理，下次上电设置与组信息保持原样

### Requirement: Settings 读写通过 StorageService
Settings App 的所有设置读取 SHALL 通过 `StorageService::load_settings`，所有设置写入 SHALL 通过 `StorageService::save_settings`。Settings App SHALL NOT 直接访问 NVS 或其他存储后端。

#### Scenario: 设置变更持久化
- **WHEN** 用户在任一设置页修改值
- **THEN** 该值通过 `save_settings` 写入 NVS `sys` 命名空间，重启后可恢复

#### Scenario: 设置损坏回退默认
- **WHEN** `load_settings` 返回 `StorageError::Corrupt` 或字段非法
- **THEN** Settings App 使用 `reset_settings` 后的默认值填充各页面
