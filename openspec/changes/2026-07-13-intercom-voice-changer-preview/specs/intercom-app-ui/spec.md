## MODIFIED Requirements

### Requirement: VoiceChanger 子状态机
IntercomApp VoiceChanger 页 SHALL 维护 `VoiceChangerSubState` 子状态：`Idle` / `Recording{remain_ms, target}` / `Previewing{remain_ms, effect}`。点击档位按钮在 Idle SHALL 切到 Recording{remain_ms=3000, target=effect}；Recording 倒计时到 0 SHALL 切到 Previewing 并回放；Previewing 倒计时到 0 SHALL 切回 Idle。用户在 Previewing 阶段点击其他档位 SHALL 停止回放 + 重新 Recording 新档位。

#### Scenario: 点击档位触发录制预览
- **WHEN** 用户在 VoiceChanger 页 Idle 状态点击 High 档位按钮
- **THEN** vc_state 切到 Recording{remain_ms=3000, target=High}，audio_svc.start_capture()（8kHz 模式），UI 显示红色录制指示 + 倒计时

#### Scenario: 录制结束自动回放
- **WHEN** Recording remain_ms 推进到 0
- **THEN** audio_svc.stop_capture()，preview_buffer 经 apply_effect(buffer, High) 处理，submit_pcm 开始回放，vc_state 切到 Previewing{remain_ms=3000, effect=High}，UI 显示播放进度条

#### Scenario: 预览中切换档位重新录制
- **WHEN** 用户在 Previewing High 阶段点击 Low 档位按钮
- **THEN** 停止当前回放，vc_state 切到 Recording{remain_ms=3000, target=Low}，重新录制 3s

#### Scenario: Recording 期间其他按钮禁用
- **WHEN** vc_state == Recording，用户触摸其他档位按钮
- **THEN** hit_test 返回 None（按钮命中被禁用），dispatch 不触发任何 action；Cancel 按钮除外（命中可取消回 Idle）

### Requirement: VoiceChanger 视觉反馈
`draw_voice_changer` SHALL 按 vc_state 分支渲染：(a) Idle 显示 3 档按钮（当前 settings.voice_effect 高亮） + "Record 3s" 提示；(b) Recording 显示按钮 dim + 红色录制指示 + 倒计时 "Recording... 2s"；(c) Previewing 显示按钮 dim + 播放进度条 + "Previewing {effect}" 文字。Cancel 按钮 SHALL 在 Recording/Previewing 时显示。

#### Scenario: 当前选中档位高亮
- **WHEN** settings.voice_effect == Normal，VoiceChanger 页 Idle 渲染
- **THEN** Normal 按钮显示高亮边框（绿色），Low / High 按钮普通灰

#### Scenario: 录制中显示倒计时
- **WHEN** vc_state == Recording{remain_ms=2000}
- **THEN** UI 显示 "Recording... 2s" + 红色圆点闪烁，每 tick 刷新倒计时数字

### Requirement: 通话中禁用预览
当 `ptt.state != Idle`（通话进行中）用户进入 VoiceChanger 页 SHALL 禁用档位按钮 + 显示 "Cannot preview during call" 文字。SHALL NOT 在通话中触发 capture（与通话 capture 冲突）。

#### Scenario: 通话中禁用档位
- **WHEN** 用户正在 PTT 通话（ptt.state=Talking），切到 VoiceChanger 页
- **THEN** 3 档按钮显示为 dim + 不可点击，中央显示 "Cannot preview during call"，避免与通话 capture 冲突
