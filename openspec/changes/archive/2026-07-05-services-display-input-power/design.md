## Context

change 02 已完成 BSP 驱动层，暴露 LCD（ST7789 SPI）、Touch（CST816 I2C）、Buttons（BOOT=GPIO9 / PLUS=GPIO18）、ADC（BAT_ADC=GPIO0）、Backlight（GPIO6）的初始化句柄。change 01 已提供 `BoardProfile` 编译期常量（引脚号、默认亮度 80、默认熄屏 30s、BACKLIGHT_PWM_SUPPORTED=true、SUPPORTS_TRUE_POWER_OFF=false）。

技术设计 §3.6 已给出三个 trait 的签名与枚举定义。PRD §7 定义按键行为规则（BOOT=PTT + 唤醒 / PLUS=音量+静音 / PWR=不参与普通交互）。PRD §9 + 技术设计 §16 定义熄屏与电源管理规则（30s 熄屏、首触只亮屏、对讲模式熄屏不阻断收发、PA 待机时关闭）。PRD §2.4 + 技术设计 §16.3 定义电池约束（ADC 平滑 + 滞回、4 档图标、避开发射/大音量采样、不做真关机）。

目录结构遵循技术设计 §20：`src/services/display.rs`、`src/services/input.rs`、`src/services/power.rs`。

## Goals / Non-Goals

**Goals:**
- 三个 Service trait + impl 全部可编译、可实例化，在设备上正常运行
- `DisplayService`：通过 LEDC PWM 控制 GPIO6 背光亮度（0-255），`screen_on` / `screen_off` 管理屏幕开关状态，`present` 接受 slint 渲染指令提交到 ST7789 framebuffer
- `InputService`：注册回调后持续分发 `InputEvent`；BOOT 按下 >= 50ms 触发 `BootPress`、< 50ms 释放触发 `BootShortTap`；PLUS 短按/长按区分；PWR 短按触发 `PowerShortPress`；CST816 触摸产生 `Touch(TouchEvent)`
- `PowerService`：ADC 读取电池电压、平滑 + 滞回输出 0-100 与 4 档图标；`enter_standby` 关屏 + 关 PA + 低功耗；`wakeup` 恢复；`reset_reason` 返回启动原因枚举；`abnormal_boot_count` 从 NVS 读取
- 熄屏首触只唤醒不触发点击；对讲模式熄屏不阻断收发
- `ResetReason` 枚举覆盖 PowerOn / Brownout / Wdt / Panic / Unknown

**Non-Goals:**
- 完整低电保护策略（仅架构预留，PRD §2.4 明确不做）
- 软件真关机（硬件不支持，`SUPPORTS_TRUE_POWER_OFF = false`）
- 充电状态检测（PRD §2.4 明确不作为正式能力）
- OTA 下载与固件更新（仅分区预留）
- 实际的 App/Shell UI 页面渲染（change 07/13）
- 音频采集/播放/编解码（change 05）
- 网络通信与组网（change 06/08/09）
- 电池百分比精确校准（仅 4 档图标，不强推百分比）

## Decisions

### D1：背光控制 = LEDC PWM（GPIO6）
`DisplayService::set_brightness(v: u8)` 通过 ESP-IDF LEDC 驱动输出 PWM 到 GPIO6，duty cycle 映射 0-255 → 0-100% 占空比。`screen_off` 将 duty 设为 0；`screen_on` 恢复至当前亮度（默认 `BoardProfile::DEFAULT_BRIGHTNESS = 80`）。备选：GPIO 直接 on/off 控制背光——丢失调光能力，PRD 要求可调亮度，排除。

### D2：present = slint 后端渲染提交
`present(&self, view: &DrawCmd)` 接受 `DrawCmd` 枚举，当前定义为 `DrawCmd::SlintUpdate`（触发 slint 后端刷新到 ST7789 framebuffer）。slint 的 `slint::Window` 在 `set_brightness` / `screen_on/off` 时保持窗口句柄不变，仅控制背光。备选：直接 `embedded-graphics` 绘制——change 01 D3 已留退出口，但本期仍用 slint 作为主后端，与 change 01 决策一致。

### D3：InputService 事件分发 = 回调注册模式
`on_event(cb: Box<dyn Fn(InputEvent) + Send + Sync>)` 注册单一回调，内部在 BSP 驱动的中断/轮询线程中收集事件后调用回调。备选：channel 模式——回调更直接、延迟更低，PTT 实时性要求高；channel 模式作为 change 10 的 IntercomService 层选择，本 Service 层暴露回调即可。

### D4：BOOT PTT 阈值 = 50ms
BOOT 按下后启动 50ms 计时器：若 50ms 内释放 → `BootShortTap`（唤醒屏幕）；若 >= 50ms 仍在按下 → 发出 `BootPress { screen_was_off }`（进入 PTT 准备，携带按下时刻屏幕是否熄灭的状态），后续释放时发出 `BootRelease`。技术设计 §11 明确要求"按压触发阈值，避免误触短按直接发言"。备选：无阈值——PRD §7.2 明确要求阈值，排除。`screen_was_off` 字段供 change 10 决定熄屏 PTT 时不唤醒屏幕（D45 互补）。

### D5：PLUS 短/长按区分 = 阈值 500ms
PLUS 按下后启动 500ms 计时器：< 500ms 释放 → `PlusShortPress`（音量面板）；>= 500ms 仍在按下 → `PlusLongPress`（静音 toggle），释放时不额外发事件。备选：双击检测——PRD §7.1 未要求双击，排除。

### D6：PWR 短按 = 软件亮/熄屏 toggle，长按 = 硬件关机
PWR 短按（< 2s 释放）→ `PowerShortPress`，由上层决定亮/熄屏 toggle。PWR 长按由 ESP-IDF bootloader 直接处理硬件关机，不经软件。PRD §2.5 + §7.1 明确"PWR 不参与普通交互，长按关机"。

### D7：触摸熄屏首触 = 仅唤醒，丢弃首次 Touch 事件
当 `is_screen_on() == false` 时，CST816 首次触摸事件仅触发 `screen_on()`，不向回调分发 `Touch(TouchEvent)`。PRD §7.3 + §9.2 明确"熄屏首触只亮屏，不触发点击"。第二次触摸正常分发。

### D8：电池采样 = 平滑窗口 + 电压阈值滞回档位
ADC 读取原始值（BSP 仅提供 raw ADC，不做映射）→ 转换电压（除以 `BoardProfile::BAT_ADC_DIVIDER` ×3 分压还原）→ 映射 0-100 → 指数移动平均平滑。4 档映射基于电压阈值（D4 单一数据源）：< 3.4V → 1 bar（critical）、3.4–3.6V → 2 bars（low）、3.6–3.9V → 3 bars（medium）、> 3.9V → 4 bars（full）。档位切换需穿越 ±0.1V 滞回带避免跳变。采样周期 2s，但在音频发射/大音量播放期间暂停采样避免瞬态干扰（PRD §9.1 + 技术设计 §16.3）。备选：固定查表无平滑——PRD 明确要求平滑 + 滞回，排除。

### D9：enter_standby = 熄屏 + 关 PA + 保持对讲收发能力
`enter_standby()` 调用 `DisplayService::screen_off()` + 关闭 PA 控制引脚（GPIO15）+ 降低 CPU 频率（可选）。不关闭 Wi-Fi/ESP-NOW，对讲模式熄屏不阻断收发（PRD §9.2 + 技术设计 §16.2）。`wakeup()` 恢复屏幕与 PA。进入 Listening 前软启动 PA 避免爆音（技术设计 §16.2）。

### D10：reset_reason = ESP-IDF RTC reset cause 映射
`reset_reason()` 读取 `esp_idf_svc::hal::reset` 的 reset reason，映射到 `ResetReason` 枚举：`PowerOn`（上电复位）、`Brownout`（掉电复位）、`Wdt`（看门狗）、`Panic`（panic 重启）、`Unknown`（其他）。`abnormal_boot_count()` 从 NVS 读取 `StorageService::load_diag().abnormal_boot_cnt`（依赖 change 03 的 StorageService trait，本变更通过依赖注入获取）。

### D11：DrawCmd 枚举定义
```rust
pub enum DrawCmd {
    SlintUpdate,           // 触发 slint 后端刷新
    Clear,                 // 清屏（全黑）
    RawFramebuffer(&'static [u8]), // 直接写入 framebuffer（预留，本期不用）
}
```
当前实现仅支持 `SlintUpdate`，`RawFramebuffer` 为后续可能的 embedded-graphics 退出口预留。

## Risks / Trade-offs

- **[CST816 触摸中断延迟]** → ESP-IDF I2C 中断响应可能在 CPU 负载高时延迟；缓解：InputService 内部使用独立线程轮询 I2C，不依赖中断上下文回调
- **[PTT 50ms 阈值可能在低功耗待机下不准]** → 待机时 CPU 降频，计时器精度下降；缓解：使用 ESP-IDF 定时器硬件计数而非软件 sleep
- **[ADC 采样在音频发射期间受干扰]** → D8 已通过暂停采样缓解；若仍有跳变，增加 RC 滤波软件层
- **[slint 渲染在无 PSRAM 上的帧率]** → present 仅触发增量刷新，不做全帧重绘；change 01 D3 已留 embedded-graphics 退出口
- **[PWR 短按与长按的边界]** → 2s 阈值由 ESP-IDF bootloader 控制，软件层仅处理 < 2s 的短按事件；若 bootloader 阈值不同，以实际硬件行为为准
- **[abnormal_boot_count 依赖 change 03 StorageService]** → 若 change 03 未完成，本变更可临时返回 0 并标注 TODO；但 change 03 应在本变更之前完成（无依赖关系冲突，仅运行时实例化顺序）

## Migration Plan

无既有运行时需要迁移。部署步骤：
1. 确保 change 02 的 BSP 驱动已合入（LCD/Touch/Buttons/ADC/Backlight 句柄可用）
2. 确保 change 03 的 StorageService trait 已定义（用于 `abnormal_boot_count` 读取）
3. 新增三个 Service 文件，在 `services/mod.rs` 注册
4. 编译验证：`cargo build` 零错误
5. 烧录验证：屏幕可亮/灭、触摸可唤醒、按键可触发事件回调、电量图标显示正确

回滚：`git revert <commit>`，移除三个 Service 文件与 `mod.rs` 注册行。

## Open Questions

无。所有技术决策已与用户在 explore 阶段对齐，trait 签名与枚举定义严格遵循技术设计 §3.6，行为规则遵循 PRD §7/§9/§2.4。
