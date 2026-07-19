## MODIFIED Requirements

### Requirement: PTT 区视觉反馈
`src/apps/view/intercom_view.rs` PTT 区 SHALL 按 `IntercomApp.ptt.state()` 分支 draw：`Idle` 状态绘制为蓝灰色按钮 + "PTT" 文字；`Talking` 状态绘制为绿色按钮 + "正在说话" + 波形图标；`Cooldown` 状态绘制为黄色按钮 + 剩余冷却倒计时。`ChannelBusy` 时 Idle 状态 SHALL 绘制为半透明灰视觉降权（提示但不阻止按 PTT）。

#### Scenario: Talking 时按钮变绿
- **WHEN** 用户触摸 PTT Down，VoicePttMachine 转 Talking
- **THEN** 下一 tick 渲染 PTT 区为绿色按钮 + "正在说话" 文字 + 波形图标，dirty=true

#### Scenario: Cooldown 倒计时
- **WHEN** 用户释放 PTT，VoicePttMachine 转 Cooldown（如 1s 冷却）
- **THEN** PTT 区黄色 + 显示 "1s" 倒计时数字，每 tick 刷新；冷却结束回 Idle

#### Scenario: ChannelBusy 视觉降权
- **WHEN** 远端 peer 正在说话触发 ChannelBusy，本地 PTT 在 Idle
- **THEN** PTT 区绘制为半透明灰，提示当前有冲突；用户仍可按 PTT（不阻止），main loop log::warn

### Requirement: 远端 peer 卡片"正在说话"指示
`draw_peer_card` SHALL 检查 `peer.voice_state == TalkState::Talking`，若是则卡片边框变绿 + 显示波形图标。本地用户正在说话（ptt.state == Talking）SHALL 在本端 peer 卡片（如有自显示）也显示同样指示。

#### Scenario: 远端 peer A 正在说话
- **WHEN** on_peer_voice_state(A, Talking) 更新 peer A voice_state
- **THEN** A 卡片边框变绿色 + 波形图标显示

#### Scenario: 本地正在说话不影响远端卡片
- **WHEN** 本地用户按 PTT（ptt.state = Talking），远端 peer 卡片 SHALL 保持原状态（不应被本地状态污染）
- **THEN** 远端 peer 卡片颜色不变，仅本地 PTT 区显示 Talking 绿色
