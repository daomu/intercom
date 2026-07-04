## 1. voice.rs 骨架与 VoiceState 枚举

- [ ] 1.1 新增 `src/intercom/voice.rs`：定义 `pub enum VoiceState { Idle, Talking, Listening, ChannelBusy }`，`derive(Debug, Clone, Copy, PartialEq, Eq)`
- [ ] 1.2 在 `src/intercom/mod.rs` 追加 `pub mod voice;` 与 `pub use voice::VoiceState;` 重导出
- [ ] 1.3 定义 `VoiceMachine` 结构体持有当前 state、mode、group_peers（mac+id 列表，来自 change 09）、self_id、audio/network service 引用、`capture_armed: AtomicBool`、`conflict_flag: AtomicBool`、`arb_condvar: Condvar`（+ 配套 Mutex）、voice seq 计数器

## 2. 状态迁移与半双工互斥

- [ ] 2.1 实现 `fn transition(&mut self, target: VoiceState)`：根据 target 调用 AudioService 接口落实半双工互斥——Talking: stop_playback + start_capture；Listening: stop_capture + start_playback（或接收链 ON）；Idle: stop_capture + 接收链 ON standby；ChannelBusy: 采集 OFF + 接收 ON standby
- [ ] 2.2 实现 `fn current_state(&self) -> VoiceState` 查询接口
- [ ] 2.3 验证迁移规则：仅允许 Idle→Talking、Idle→Listening、Idle→ChannelBusy、Talking→Idle、Listening→Idle、ChannelBusy→Idle；非法迁移返回错误或 panic-log

## 3. PTT press 流程

- [ ] 3.1 实现 `fn ptt_press(&mut self) -> Result<(), IntercomError>`：仅 `Grouped(Idle)` 时执行，其他状态返回 `Busy`
- [ ] 3.2 clear-mode 分支：调用 change 08 packet.rs `encode_talk_state(action=1)` 构造 TALK_STATE → 对所有 peer send_unicast → `conflict_flag.store(false)` → `Condvar::wait_timeout(50ms)`（ESP-NOW recv 回调解析 incoming TALK_STATE，收到 action=1 则 `conflict_flag.store(true)` + `condvar.notify()`）；被唤醒后读 conflict_flag：true → transition(ChannelBusy) + play_busy_tone + return Ok（不进 Talking）；timeout → 无冲突继续
- [ ] 3.3 free-mode 分支：`encode_talk_state(action=1)` 构造 TALK_STATE → send_unicast（不等待仲裁）→ 继续 Talking 流程
- [ ] 3.4 Talking 进入流程：`transition(Talking)` → `audio.pa_enable(true)` → `audio.start_capture()` → `play_ready_tone()` → `std::thread::sleep(保护间隔约30ms)` → `capture_armed.store(true)`
- [ ] 3.5 注册 `audio.on_capture_frame` 回调：仅 `capture_armed=true` 时 `encode_voice` 构造 VOICE 包（调用 change 08 `encode_voice`，seq 递增）→ 对所有非自身 peer `network.send_unicast`

## 4. PTT release 流程

- [ ] 4.1 实现 `fn ptt_release(&mut self)`：`capture_armed.store(false)` → `audio.stop_capture()` → `audio.pa_enable(false)`
- [ ] 4.2 若曾进入 Talking（state==Talking）：`encode_talk_state(action=0)` 构造 TALK_STATE → send_unicast（clear/free-mode 均发）
- [ ] 4.3 若 state==ChannelBusy：不发 TALK_STATE(action=0)（未占用频道）
- [ ] 4.4 `transition(Idle)`
- [ ] 4.5 顺序遵循技术设计 §19.4：stop_capture → pa_enable(false) → send TALK_STATE(action=0)（仅曾进 Talking） → state=Grouped(Idle)

## 5. 接收侧状态迁移（Listening 入口）

- [ ] 5.1 实现 `fn on_recv_voice(&mut self, pkt)`：若 state==Idle 则 transition(Listening)；若 state==Talking 则忽略（半双工，自身发言时不收）
- [ ] 5.2 实现 `fn on_recv_talk_state(&mut self, pkt)`：若 state 处于 clear-mode Condvar 仲裁等待窗口内且收到 action=1 → `conflict_flag.store(true)` + `condvar.notify()` 唤醒等待方；其他情况供 UI 展示（VoiceActive 事件）
- [ ] 5.3 Listening 态的 jitter/解码/混音/播放留给 change 11，本变更仅确保状态迁移与接收链 ON

## 6. BOOT 按键去抖与路由

- [ ] 6.1 在 InputService 实现（或 Shell 层）BOOT 按下计时：≥50ms 发 `BootPress`，<50ms 发 `BootShortTap`
- [ ] 6.2 Shell 层路由：`BootPress` → 检查 `IntercomService::state()` 为 Grouped 且服务可用 → 调用 `ptt_press()`；未组网则忽略
- [ ] 6.3 Shell 层路由：`BootRelease` → 若当前在 Talking/ChannelBusy → 调用 `ptt_release()`
- [ ] 6.4 Shell 层路由：`BootShortTap` → 调用 `DisplayService::screen_on()`（若熄屏）

## 7. 全局跨 App PTT 路由

- [ ] 7.1 确认 BOOT 事件路由在 Shell/InputService 全局层处理，不依赖各 App 自行注册
- [ ] 7.2 验证：前台 App 为 Settings/Launcher 时，BOOT 长按仍触发 ptt_press

## 8. 熄屏 PTT 直通

- [ ] 8.1 InputService 在 `is_screen_on()==false` 时对 BOOT ≥50ms 仍发 `BootPress { screen_was_off: true }`（字段定义于 change 04，本变更不修改 InputEvent 枚举）
- [ ] 8.2 IntercomService 收到带 `screen_was_off=true` 的 BootPress → 正常进 ptt_press 流程，不调用 `DisplayService::screen_on`
- [ ] 8.3 熄屏短按（<50ms）仍发 `BootShortTap` → 仅亮屏，不触发 PTT

## 9. 触摸 PTT 备用入口

- [ ] 9.1 在 intercom_app 主页面定义 PTT 触摸区域（slint 组件，大面积触摸区）
- [ ] 9.2 touch-down → `IntercomService::ptt_press()`（无 50ms 去抖）
- [ ] 9.3 touch-up → `IntercomService::ptt_release()`
- [ ] 9.4 触摸 PTT 与 BOOT PTT 共用同一 ptt_press/ptt_release，行为一致

## 10. 就绪提示音与忙态提示音

- [ ] 10.1 实现 `play_ready_tone()`：调用 AudioService 播放短就绪提示音（在 start_capture 之后、capture_armed=true 之前完成）
- [ ] 10.2 实现 `play_busy_tone()`：调用 AudioService 播放忙态提示音（ChannelBusy 态触发）
- [ ] 10.3 验证 ready tone 不混入 VOICE 包发送（capture_armed 标志保证）

## 11. clear-mode 近同时抢话退化

- [ ] 11.1 验证 50ms 仲裁窗口不作为严格互斥锁：A、B 近同时抢话可能都进 Talking（已知可接受退化）
- [ ] 11.2 验证 release 时发 TALK_STATE(action=0)，下一轮 PTT 抢话仲裁窗口重新生效

## 12. 编译与单元验证

- [ ] 12.1 `cargo build` 零编译错误
- [ ] 12.2 `cargo build --release` 通过
- [ ] 12.3 状态迁移规则单元测试（可用 host-side 逻辑测试，不依赖硬件）

## 13. 烧录验收

- [ ] 13.1 烧录 2 台已组网设备（组关系由 change 09 建立）
- [ ] 13.2 clear-mode：A 按住 BOOT → 听到 ready tone → B 收到 TALK_STATE → A 讲话 → B 状态迁移到 Listening（解码/播放待 change 11）
- [ ] 13.3 clear-mode：A 发言中 B 按 BOOT → B 听到 busy tone，不进 Talking
- [ ] 13.4 clear-mode：A 松开 BOOT → B 收到 TALK_STATE(action=0) → B 回 Idle
- [ ] 13.5 free-mode：A、B 几乎同时按 BOOT → 都进 Talking（无 busy tone）
- [ ] 13.6 熄屏长按 BOOT → 直接进 Talking，屏幕不亮
- [ ] 13.7 前台为 Settings App 时 BOOT 长按 → 触发 ptt_press
- [ ] 13.8 触摸 PTT 区域按下/松开 → 行为与 BOOT 一致

## 14. 收尾

- [ ] 14.1 提交 commit：`feat: implement voice state machine + PTT flow (change 10/17)`
- [ ] 14.2 在 commit message 注明接收链 jitter/混音部分留给 change 11 接续
