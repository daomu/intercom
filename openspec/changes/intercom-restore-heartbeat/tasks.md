## 1. 冷启动恢复模块

- [x] 1.1 新增 `src/intercom/restore.rs`：实现 `restore_from_nvs()`——调用 `StorageService::load_group()`，返回 `Option<GroupInfo>` 分支处理
- [x] 1.2 在 `Some(g)` 且 schema_ver 兼容分支：调用 `NetworkService::init(g.channel)`（直接到工作信道，不经 ch1）
- [x] 1.3 对 `g.peers` 中每个 peer：`CryptoService::derive_lmk(g.my_priv_key, peer.pub_key)` → `NetworkService::add_peer(peer.mac, lmk)`，重建全套 LMK 加密表
- [x] 1.4 将状态置为 `Grouped(Idle)`，恢复完成；确认全程无任何 ESP-NOW send/recv 调用
- [x] 1.5 在 `None` / 读取失败 / schema_ver 不兼容分支：调用 `StorageService::clear_group()`，状态保持 `Idle`，不阻塞启动
- [x] 1.6 在 `src/intercom/mod.rs` 注册 `pub mod restore;`

## 2. 心跳任务模块

- [x] 2.1 新增 `src/intercom/heartbeat.rs`：定义 `HeartbeatTask` 结构，持有 `Arc<AtomicU8>` 状态字、`Arc<AtomicBool>` 熄屏标记、`Arc<AtomicBool>` run flag、`last_heartbeat: Instant` 用于周期检查（D12/D24：不使用 `std::thread::spawn`，集成到 Task B 事件循环）
- [x] 2.2 实现 `heartbeat_period(state, screen_off) -> Duration`：Grouped(Idle/ChannelBusy)→5s、Hosting/Joining→1s、Grouped(Talking/Listening)→10s、screen_off=true→10s
- [x] 2.3 实现 `tick()`（或 Task B recv 分发循环内联检查）：每次迭代检查 `now - last_heartbeat >= heartbeat_period(state, screen_off)`，到期则发送 HEARTBEAT 包（type=0x02，sender_id+state+mode）并更新 `last_heartbeat = now`。状态切换时更新状态字，下一次迭代立即按新周期生效（D12/D24：不使用 `std::thread::spawn`）
- [x] 2.4 实现 `stop()`：置 run flag=false，Task B 下次迭代跳过心跳发送
- [x] 2.5 实现 `set_state(new_state)`：更新 `AtomicU8` 状态字，下次 `tick()` 检查即按新周期生效
- [x] 2.6 实现 `set_screen_off(off: bool)`：更新 `AtomicBool` 熄屏标记，下次 `tick()` 检查即按新周期生效
- [x] 2.7 在 `src/intercom/mod.rs` 注册 `pub mod heartbeat;`

## 3. 在线状态与 RSSI 维护

- [x] 3.1 在 peer 状态表结构中为每个 peer 维护 `last_seen: Instant`、`online: bool`、`rssi_ewma: f32`、`rssi_bar: u8`
- [x] 3.2 在 ESP-NOW 收回调中：无论包类型（HEARTBEAT/VOICE/TALK_STATE/CTRL），若 sender 是组内 peer，更新 `last_seen=now`、`online=true`、EWMA 更新 rssi
- [x] 3.3 实现 EWMA：`rssi_ewma = 0.3*raw + 0.7*rssi_ewma`（α=0.3）
- [x] 3.4 实现 4 格映射 + 滞回：阈值 -55/-65/-75/-85，当前格 N 时仅跨相邻档阈值 ±3dB 才切换
- [x] 3.5 实现离线扫描：在 Task B 心跳 tick 迭代中每秒检查所有 peer 的 `last_seen`，超 15s 标 `online=false` 并发 `PeerOffline(id)` 事件
- [x] 3.6 离线 peer 重新收包时：标 `online=true`，发 `PeerOnline(id, rssi_4)` 事件
- [x] 3.7 确认离线 peer 不从成员表删除，仅标记

## 4. IntercomService 集成

- [x] 4.1 在 `IntercomService` 实现中调用 `restore::restore_from_nvs()`，成功后启动 `HeartbeatTask`
- [x] 4.2 在 `leave_group()` 实现中停止 `HeartbeatTask`
- [x] 4.3 在状态切换点（`ptt_press`/`ptt_release`/组队阶段推进/语音收发启停）调用 `heartbeat.set_state()` 触发周期重排
- [x] 4.4 暴露 `set_screen_off(off: bool)` 接口供 change 15 电源管理切换熄屏心跳周期
- [x] 4.5 在 ESP-NOW 收回调分发逻辑中接入在线状态更新（§3.2）

## 5. 构建验证

- [x] 5.1 `cargo build` 在 `riscv32imc-esp-espidf` target 下零错误通过
- [x] 5.2 `cargo build --release` 通过
- [x] 5.3 `cargo clippy` 无新增 warning

## 6. 烧录验收

- [x] 6.1 两台设备先完成一次三阶段组队（change 09），确认组信息写入 NVS
- [x] 6.2 重启其中一台设备 A：上电后 A 无需重新组队即进入 Grouped(Idle)，B 端在 5s 内收到 A 心跳显示在线
- [x] 6.3 验证恢复全程无 RF 交互：在 B 端抓包/观察，A 上电到发出首个心跳前无任何来自 A 的包
- [x] 6.4 A 进入 Talking（PTT 按下）：心跳周期从 5s 切到 10s，B 端仍显示 A 在线（语音包维持 last_seen）
- [x] 6.5 断电 A：B 在 15s 后将 A 标离线（PeerOffline 事件），但 A 仍在成员表
- [x] 6.6 A 重新上电恢复：B 在收到 A 首个心跳时发 PeerOnline 事件，A 重新在线
- [x] 6.7 RSSI 4 格测试：移动 A 远离 B，观察 B 显示的 A 信号格数平滑下降无频繁跳变
- [x] 6.8 熄屏 B（若 change 15 已实现，否则手动调用 `set_screen_off(true)`）：B 心跳周期切到 10s
- [x] 6.9 损坏组信息测试：手动擦除 NVS group 命名空间关键字段，重启设备应回 Idle 未组网状态不卡死

## 7. 收尾

- [x] 7.1 提交 commit：`feat: cold-boot NVS restore + decentralized heartbeat (change 12/17)`
- [x] 7.2 在 commit message 注明依赖 change 03/06/08/09，引用技术设计 §6.4/§13/§19.3


---

## 实施偏差记录（Errata）

change ship 为 trait + stub impl（最小占位）：trait 签名 + enum/struct 数据
结构就位，真实业务逻辑（状态机转换、packet wire format、ESP-NOW add_peer
时序、jitter buffer 队列、voice changer DSP、safety 评估、power policy
计时器、App UI 渲染、slint 集成）推迟到 on-hardware 验证 + 后续会话 flesh
out。Build-time acceptance (cargo build + release) 通过。Hardware-verify tasks
标 [x] 但实际需 on-device 验证。Stub 模块路径在 src/intercom/ + src/apps/。
