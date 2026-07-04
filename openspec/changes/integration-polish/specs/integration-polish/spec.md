## ADDED Requirements

### Requirement: 端到端全流程贯通
2 台设备 SHALL 能在一次连续操作中走通完整流程：boot → 创建主机 → 搜索 → 加入 → 三阶段组队（PAID/JOIN/DIRECTORY_BROADCAST + 信道切换 + LMK 重构） → PTT 通话 → 松开 → 对方 PTT → 熄屏 → 熄屏收语音 → 熄屏 PTT → reboot → 冷启动 restore，全程无状态机卡死、无 panic、无空中交互恢复失败。

#### Scenario: 2 设备端到端全流程一次走通
- **WHEN** 设备 A 创建主机选模式，设备 B 搜索到 A 并加入，三阶段完成后 A 与 B 互按 PTT 通话，A 熄屏后 B PTT 通话 A 收语音，A 熄屏按 PTT 发言，A reboot 后冷启动从 NVS 恢复组关系
- **THEN** 全流程无卡死/无 panic；冷启动后 A 仍处于已组网待机，可立即 PTT 通话；B 端成员列表显示 A 在线

### Requirement: PRD §26.1 系统架构验收
固件 SHALL 具备 App + Service + BSP 三层分层，BoardProfile 为轻量只读常量；对讲与 Settings 明确分离；未实现功能 SHALL NOT 在 Launcher 暴露空入口。

#### Scenario: 三层分层与入口暴露核验
- **WHEN** 审查代码目录结构与 Launcher 页面
- **THEN** `src/apps/`、`src/services/`、`src/hal/` 分层存在；Launcher 仅暴露对讲与 Settings；无空入口占位

### Requirement: PRD §26.2 组队三阶段 ECDH 验收
A 创建主机选模式后，B SHALL 在 1 信道搜索到 A；B 加入过程 SHALL 可见模式/成员列表/人数/信号/MAC 后 4 位；A 确认组队后列表冻结并下发通讯录+切换时间戳；全员 SHALL 切目标信道并后台 ECDH 重构两两 LMK 后加密对讲打通；A/B 本地 NVS SHALL 持久化私钥+公钥总表+信道+模式；组队成功后 SHALL NOT 显示 Host/Join 身份。

#### Scenario: 三阶段组队加密对讲打通
- **WHEN** A 创建主机选清晰模式，B 在 1 信道搜索加入，A 确认组队
- **THEN** 全员切目标信道；A 与 B 互发 VOICE 包可加密解码；A/B NVS 含私钥+公钥总表+信道+模式；UI 不再显示 Host/Join 身份

### Requirement: PRD §26.3 模式验收
清晰模式下同时刻 SHALL 仅一人发言，其他人按 PTT 收到忙态反馈，近同时竞争允许短时退化但应恢复单人占用；自由模式下 A 与 B 可同时发言，C 在良好环境 SHALL 可混音接收至少 2 路远端语音，UI 逐个显示发言成员，本机发言时不播放他人音频。

#### Scenario: 清晰模式忙态反馈
- **WHEN** A 正在 PTT 发言，B 按 PTT
- **THEN** B 收到频道忙态反馈，不进入 Talking

#### Scenario: 自由模式 2 路混音
- **WHEN** A 与 B 同时 PTT 发言，C 处于自由模式已组网
- **THEN** C 同时混音接收 A 与 B 两路语音；UI 逐个显示 A、B 发言图标；C 按 PTT 时不播放 A/B 音频

### Requirement: PRD §26.4 退出与成员状态验收
本机退出后 SHALL 清理本地 NVS（私钥+公钥总表+组信息）；其他成员后续 SHALL NOT 收到其消息并心跳标离线；离线成员 SHALL NOT 自动删除；成员变化需整组重建。

#### Scenario: 成员退出与离线标记
- **WHEN** B 主动退出群组
- **THEN** B 本地 NVS 组信息已清空；A 后续收不到 B 消息，A 的成员列表显示 B 离线且不自动删除

### Requirement: PRD §26.5 设置验收
设备 SHALL 可设置设备名称、随机生成名称、全局音量、静音、亮度、自动熄屏时间；恢复出厂后 SHALL 清空所有本地组信息和设置。

#### Scenario: 设置项与恢复出厂
- **WHEN** 在 Settings 页修改设备名称/音量/静音/亮度/熄屏时间并恢复出厂
- **THEN** 全部设置项持久化到 NVS；恢复出厂后 NVS 组信息与设置全部清空，设备回到默认未组网状态

### Requirement: PRD §26.6 功耗与熄屏验收
默认 30 秒自动熄屏 SHALL 生效；对讲模式熄屏后 SHALL 仍能继续收听与发言；熄屏按 PTT SHALL NOT 亮屏；收到他人语音 SHALL NOT 亮屏；触摸首触 SHALL 只唤醒不触发其他操作。

#### Scenario: 熄屏收发不亮屏
- **WHEN** 已组网设备熄屏后，对端 PTT 发语音，本机随后按 PTT
- **THEN** 收语音期间不亮屏；按 PTT 直接发言不亮屏；触摸唤醒仅亮屏不触发页面操作

### Requirement: PRD §26.7 安全验收
未组网设备 SHALL NOT 正常加入现有组；非组成员 SHALL NOT 正常解码组内通信；重启后组关系 SHALL 可恢复（NVS 冷启动免空中交互）；单成员退出 SHALL NOT 等同密码学吊销，整组重建后旧 LMK SHALL 全部失效。

#### Scenario: 非组成员无法解码
- **WHEN** 非组成员设备在目标信道嗅探组内 VOICE 包
- **THEN** 解密失败，无法得到可懂 PCM

#### Scenario: 冷启动免空中交互恢复
- **WHEN** 已组网设备 reboot
- **THEN** 从 NVS 读取 LMK/公钥表/信道，无需任何空中交互即恢复组关系并处于已组网待机

### Requirement: PRD §26.8 稳定性与长稳验收
2 台设备长时间待机后 SHALL 仍可通话；反复熄屏/唤醒后 SHALL 仍正常；连续重启恢复多次后组关系 SHALL 正确；异常数据 SHALL NOT 卡死；多设备长时间在线时成员状态 SHALL 稳定显示。

#### Scenario: 长时间待机后通话
- **WHEN** 2 台设备已组网待机 ≥2 小时后，一方 PTT
- **THEN** 对端正常收语音播放，无超时离线误判

#### Scenario: 反复熄屏唤醒
- **WHEN** 已组网设备执行 ≥20 次熄屏/唤醒循环后 PTT
- **THEN** 对讲收发正常，无状态机卡死

#### Scenario: 连续重启恢复
- **WHEN** 已组网设备连续 reboot ≥3 次
- **THEN** 每次冷启动均从 NVS 正确恢复组关系，成员列表正确

#### Scenario: 异常数据不卡死
- **WHEN** NVS 组信息字段被人为损坏后启动
- **THEN** 系统设置回退默认/组信息清理，不卡死，进入未组网状态

#### Scenario: 多设备长时间在线成员状态稳定
- **WHEN** ≥2 台设备已组网在线 ≥2 小时
- **THEN** 成员在线状态无错误抖动（无心跳超时误判离线）

### Requirement: 技术设计 §21 待验证项实机验证
技术设计 §21 风险与待验证项表的 10 项 SHALL 全部实机验证并记录结论；其中 Opus PLC 4 帧阈值、jitter 3 帧初始水位、Opus 比特率三项 SHALL 完成实测调优。

#### Scenario: Opus PLC 阈值调优
- **WHEN** 在可控丢包率（5%/10%/20%）下评估连续丢包 4 帧/80ms 静音封底的主观连续性
- **THEN** 记录调优结论；若偏离 4 帧，以最小常量 patch 回写 BoardProfile

#### Scenario: jitter 初始水位调优
- **WHEN** 在弱网下评估 3 帧/60ms 初始缓存对首字丢失与端到端延时的影响
- **THEN** 记录调优结论；若偏离 3 帧，以最小常量 patch 回写 BoardProfile

#### Scenario: Opus 比特率调优
- **WHEN** 在 RISC-V 160MHz 无 PSRAM 下测不同比特率的编码耗时与音质
- **THEN** 选定稳定优先的比特率值并以常量 patch 回写 BoardProfile

#### Scenario: Opus CPU benchmark
- **WHEN** 在 ESP32-C6 RISC-V 160MHz 单核无 PSRAM 下，测量 Opus 定点数单帧（20ms）编码耗时
- **THEN** 记录 μs/帧 + CPU 占用率；若单帧耗时 >10ms（帧长 50%），触发降级评估

#### Scenario: ESP-NOW 4 节点并发发送丢包率
- **WHEN** 4 节点全组网，6 对加密 peer 并发发送 VOICE 包
- **THEN** 记录丢包率 %；评估 ESP-NOW 加密单播在 4 节点并发下的可靠性

#### Scenario: 自由模式 2 路混音长稳
- **WHEN** 自由模式下 2 路远端语音持续混音 ≥30 分钟
- **THEN** 无爆音累积、无丢字累积、无缓冲溢出；混音路径稳定

#### Scenario: BOOT 作 PTT 上电误触防护
- **WHEN** 设备上电启动过程中按住 BOOT 键
- **THEN** 不触发 PTT 发言；上电完成后 BOOT 才进入正常 PTT 功能

#### Scenario: 熄屏待机电流与唤醒时延
- **WHEN** 设备已组网熄屏待机，测量待机电流；随后对端 PTT 发语音，测量从熄屏到音频输出时延
- **THEN** 记录待机电流 mA + 唤醒接收时延 ms；评估是否符合一期功耗预期

#### Scenario: ECDH 6 次 X25519 冷启动恢复耗时
- **WHEN** 4 节点全组网设备冷启动，从 NVS 恢复需执行 6 次 X25519（4 节点两两 LMK）
- **THEN** 记录冷启动恢复总耗时 ms；评估是否影响用户体验

#### Scenario: 信道评估算法准确性
- **WHEN** 在多信道干扰场景下（模拟或真实 2.4GHz 拥塞），触发信道评估
- **THEN** 记录算法选定信道与各信道 RSSI/利用率数据；评估选定信道合理性

#### Scenario: ESP-NOW peer 表容量验证
- **WHEN** 4 节点全组网，每节点对其他 3 节点建立加密 peer
- **THEN** ESP-NOW peer 表共 12 表项（4 × 3）不超限，加密单播/组播正常

### Requirement: PRD §20 音频焦点规则验收
对讲语音播放期间其他 App 音频 SHALL 被压制；本机发言期间 SHALL NOT 播放其他成员语音也 SHALL NOT 播放其他 App 音频；变声预览 SHALL 只能在非关键对讲状态执行；优先级 SHALL 遵循 1 对讲接收语音/发言关键提示 > 2 频道忙/就绪提示 > 3 变声预览 > 4 其他 App 音频 > 5 非关键提示音。

#### Scenario: 对讲语音压制其他音频
- **WHEN** 正在播放对讲接收语音期间触发提示音/变声预览
- **THEN** 低优先级音频被压制或暂停，对讲语音优先播放

#### Scenario: 本机发言互斥
- **WHEN** 本机处于 TALKING 状态
- **THEN** 不播放其他成员接收语音，不播放其他 App 音频

#### Scenario: 变声预览状态约束
- **WHEN** 设备处于 TALKING 或 LISTENING 关键对讲状态时尝试变声预览
- **THEN** 预览被拒绝；预览仅在非关键对讲状态（如 Idle）可执行

### Requirement: 3 设备混音与 4 设备压力测试（best-effort）
3 设备混音与 4 设备压力测试 SHALL 以 best-effort 方式覆盖：当前仅 2 台设备可用时，以 2 设备实机 + 代码分析验证；完整 3/4 设备实机验证推迟至未来硬件可用时补测。结论作为已知边界记录，不作为一期交付阻塞项。

#### Scenario: 2 设备实机 + 代码分析覆盖
- **WHEN** 仅 2 台设备可用时，A/B 组网验证加密通信；代码分析审查 3 路混音路径、6 对 LMK 全网状、peer 表 12 表项容量
- **THEN** 2 设备加密通信正常；代码分析确认 3 路混音逻辑正确、peer 表不超限；记录标注"完整 4 设备实机验证推迟至未来硬件可用时"

### Requirement: 调优 patch 回写约束
若调优确证需变更阈值，SHALL 仅修改 `BoardProfile` 的 `const` 常量（如 PLC 阈值、jitter 初始水位、Opus 比特率），SHALL NOT 修改算法逻辑或 spec 级行为契约。

#### Scenario: 调优仅改常量
- **WHEN** 调优结论确证 PLC 阈值需从 4 帧调整为 5 帧
- **THEN** 仅修改 `BoardProfile::PLC_CONSECUTIVE_LOSS_THRESHOLD` 常量值，Service 实现引用该常量自动生效，无逻辑代码改动
