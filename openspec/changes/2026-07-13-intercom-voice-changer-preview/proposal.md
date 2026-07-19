## Why

`IntercomPage::VoiceChanger` 页当前 `draw_voice_changer` 渲染了 3 档变声按钮（Low / Normal / High），但 spec §11 要求点击档位时 SHALL 录制 3 秒预览 → 用该档位效果回放 → 用户听到效果后再决定是否切换。当前点击档位按钮仅改 `Settings.voice_effect` 字段，没有预览录制/回放流程，用户无法判断变声效果。

`VoiceEffect` 枚举（change 06 intercom-state）+ `IntercomCodec::apply_effect` 接口已建好但未接通。`audio_svc.start_capture` / `submit_pcm` 在 change #3 后可用。

## What Changes

- **修改 `src/apps/intercom_app.rs`**：VoiceChanger 页子状态新增 `VoiceChangerSubState { Idle, Recording(u32), Previewing(u32) }` 字段 + `preview_buffer: Vec<i16>`（3s @ 16kHz 单声道 = 48000 samples ≈ 96KB）
- **修改 `src/apps/view/intercom_view.rs`** `draw_voice_changer`：3 档按钮 + 当前选中高亮；Recording 状态显示倒计时（3→2→1）+ 红色录制指示；Previewing 状态显示播放进度条
- **修改 `src/apps/view/intercom_view.rs`** `hit_test`：3 档按钮命中返回 `HitTarget::VoiceEffectButton(VoiceEffect)`；录制中其他按钮禁用（hit_test 返回 None 或 CancelButton）
- **修改 `src/apps/intercom_app.rs`** `dispatch`：命中档位按钮 → 若 Idle 则启动 `Recording(3000ms)` + `audio_svc.start_capture`；3s 后切 Previewing + `audio_svc.stop_capture` + apply_effect + `submit_pcm` 回放
- **修改 `src/main.rs`**：tick 推进 VoiceChanger 子状态计时；预览完成后切回 Idle；用户在预览中点其他档位按钮 SHALL 重新录制预览
- **修改 `src/intercom/codec.rs`**（若缺）：`apply_effect(&[i16], VoiceEffect) -> Vec<i16>` 简单实现（pitch shift / 时间拉伸）——本期先用 placeholder（音量增减），真实 pitch shift 留为后续

## Capabilities

### Modified Capabilities
- `intercom-app-ui`: VoiceChanger 页 SHALL 显示 3 档按钮 + 录制/预览状态 + 倒计时 + 进度条
- `voice-changer`: 档位切换 SHALL 先录制 3s 预览再用该档位回放；apply_effect 接口 SHALL 接通
