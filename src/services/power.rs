//! Power service: battery ADC + standby/wakeup + reset reason. change 04/17.
//! design D8-D10. Spec: §3.6, §9, §2.4, §16.
//!
//! Two implementations:
//! - `PowerServiceStub`: pure software state (no hardware), for host tests.
//! - `HalPowerService`: real ADC via `BatteryDriver` + real reset reason via
//!   `esp_idf_svc::hal::reset::ResetReason`, for on-device.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;

use esp_idf_svc::hal::reset::ResetReason as IdfResetReason;

use crate::board_profile::BoardProfile;
use crate::hal::battery::BatteryDriver;
use crate::services::storage::{DiagInfo, StorageService};

/// Reset reason. design D10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    PowerOn,
    Brownout,
    Wdt,
    Panic,
    Unknown,
}

impl ResetReason {
    /// Map to the u32 code persisted in `diag.last_reset_reason` (safety-diagnostics D2).
    pub fn to_code(self) -> u32 {
        match self {
            ResetReason::PowerOn => 0,
            ResetReason::Brownout => 1,
            ResetReason::Wdt => 2,
            ResetReason::Panic => 3,
            ResetReason::Unknown => 4,
        }
    }

    /// Reverse of `to_code` — used by About page to recover the ResetReason
    /// from the persisted diag code.
    pub fn from_code(code: u32) -> Self {
        match code {
            0 => ResetReason::PowerOn,
            1 => ResetReason::Brownout,
            2 => ResetReason::Wdt,
            3 => ResetReason::Panic,
            _ => ResetReason::Unknown,
        }
    }

    /// Display string for the About page (safety-diagnostics task 5.3).
    pub fn display(self) -> &'static str {
        match self {
            ResetReason::PowerOn => "正常上电",
            ResetReason::Brownout => "电压不足",
            ResetReason::Wdt => "看门狗",
            ResetReason::Panic => "异常崩溃",
            ResetReason::Unknown => "未知",
        }
    }
}

impl fmt::Display for ResetReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display())
    }
}

/// Battery 4-bar icon. design D8.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryIcon {
    Critical, // < 3.4V, 1 bar
    Low,      // 3.4–3.6V, 2 bars
    Medium,   // 3.6–3.9V, 3 bars
    Full,     // > 3.9V, 4 bars
}

/// Power service trait. §3.6 / design D8-D10.
pub trait PowerService: Send + Sync + fmt::Debug {
    fn battery_percent(&self) -> u8;
    fn battery_icon(&self) -> BatteryIcon;
    fn enter_standby(&self);
    fn wakeup(&self);
    fn reset_reason(&self) -> ResetReason;
    fn abnormal_boot_count(&self) -> u32;
}

// ---- Stub impl (no hardware, for host tests) -----------------------------

pub struct PowerServiceStub<S: StorageService> {
    storage: S,
    state: Mutex<PowerState>,
}

#[derive(Debug, Clone, Copy)]
struct PowerState {
    percent: u8,
    icon: BatteryIcon,
    standby: bool,
}

impl<S: StorageService> fmt::Debug for PowerServiceStub<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PowerServiceStub")
            .field("storage", &"StorageService")
            .finish_non_exhaustive()
    }
}

impl<S: StorageService> PowerServiceStub<S> {
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            state: Mutex::new(PowerState {
                percent: 100,
                icon: BatteryIcon::Full,
                standby: false,
            }),
        }
    }

    /// Pure-logic voltage → icon mapping with ±0.1V hysteresis.
    pub fn voltage_to_icon(current: BatteryIcon, voltage: f32) -> BatteryIcon {
        let hyst = 0.1;
        match current {
            BatteryIcon::Full => {
                if voltage < 3.9 - hyst {
                    BatteryIcon::Medium
                } else {
                    BatteryIcon::Full
                }
            }
            BatteryIcon::Medium => {
                if voltage < 3.6 - hyst {
                    BatteryIcon::Low
                } else if voltage >= 3.9 + hyst {
                    BatteryIcon::Full
                } else {
                    BatteryIcon::Medium
                }
            }
            BatteryIcon::Low => {
                if voltage < 3.4 - hyst {
                    BatteryIcon::Critical
                } else if voltage >= 3.6 + hyst {
                    BatteryIcon::Medium
                } else {
                    BatteryIcon::Low
                }
            }
            BatteryIcon::Critical => {
                if voltage >= 3.4 + hyst {
                    BatteryIcon::Low
                } else {
                    BatteryIcon::Critical
                }
            }
        }
    }
}

impl<S: StorageService> PowerService for PowerServiceStub<S> {
    fn battery_percent(&self) -> u8 {
        self.state.lock().expect("power state").percent
    }
    fn battery_icon(&self) -> BatteryIcon {
        self.state.lock().expect("power state").icon
    }
    fn enter_standby(&self) {
        let mut s = self.state.lock().expect("power state");
        s.standby = true;
    }
    fn wakeup(&self) {
        let mut s = self.state.lock().expect("power state");
        s.standby = false;
    }
    fn reset_reason(&self) -> ResetReason {
        ResetReason::PowerOn
    }
    fn abnormal_boot_count(&self) -> u32 {
        let diag: DiagInfo = self.storage.load_diag();
        diag.abnormal_boot_cnt
    }
}

unsafe impl<S: StorageService + Send> Send for PowerServiceStub<S> {}
unsafe impl<S: StorageService + Send> Sync for PowerServiceStub<S> {}

// ---- Real HAL impl (ADC + reset reason) ----------------------------------

/// Real power service backed by `BatteryDriver` (ADC) + ESP-IDF reset reason.
/// EWMA smoothing (α=0.3) applied to raw ADC samples for stable readings.
pub struct HalPowerService<S: StorageService> {
    storage: S,
    battery: Mutex<BatteryDriver>,
    state: Mutex<PowerState>,
    /// EWMA-smoothed raw ADC value (0..=4095).
    ewma_raw: Mutex<u16>,
}

impl<S: StorageService> fmt::Debug for HalPowerService<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HalPowerService")
            .field("storage", &"StorageService")
            .finish_non_exhaustive()
    }
}

impl<S: StorageService> HalPowerService<S> {
    pub fn new(storage: S, battery: BatteryDriver) -> Self {
        Self {
            storage,
            battery: Mutex::new(battery),
            state: Mutex::new(PowerState {
                percent: 100,
                icon: BatteryIcon::Full,
                standby: false,
            }),
            ewma_raw: Mutex::new(0),
        }
    }

    /// Convert raw 12-bit ADC (0..=4095) to cell voltage via ×3 divider.
    /// ADC range 0..=4095 ≈ 0..=3.1V (12dB attenuation). Battery voltage =
    /// ADC_voltage × divider (3). So cell = (raw / 4095) × 3.1 × 3.
    fn raw_to_voltage(raw: u16) -> f32 {
        (raw as f32 / 4095.0) * 3.1 * (BoardProfile::BAT_ADC_DIVIDER as f32)
    }

    /// Convert cell voltage to percent (0..=100). Linear 3.0V→0%, 4.2V→100%.
    fn voltage_to_percent(v: f32) -> u8 {
        let pct = ((v - 3.0) / (4.2 - 3.0) * 100.0).clamp(0.0, 100.0);
        pct as u8
    }

    /// Sample the ADC, apply EWMA smoothing, and update state.
    /// Called periodically by the caller (e.g., every 1s).
    pub fn sample(&self) {
        let raw = if let Ok(mut bat) = self.battery.lock() {
            bat.read_raw_adc().unwrap_or(0)
        } else {
            return;
        };
        // EWMA: new = α×raw + (1-α)×old, α=0.3
        let mut ewma = self.ewma_raw.lock().unwrap();
        *ewma = ((*ewma as f32 * 0.7) + (raw as f32 * 0.3)).round() as u16;
        let voltage = Self::raw_to_voltage(*ewma);
        let percent = Self::voltage_to_percent(voltage);

        let mut state = self.state.lock().unwrap();
        state.percent = percent;
        state.icon = PowerServiceStub::<S>::voltage_to_icon(state.icon, voltage);
    }
}

impl<S: StorageService> PowerService for HalPowerService<S> {
    fn battery_percent(&self) -> u8 {
        self.state.lock().unwrap().percent
    }
    fn battery_icon(&self) -> BatteryIcon {
        self.state.lock().unwrap().icon
    }
    fn enter_standby(&self) {
        let mut s = self.state.lock().unwrap();
        s.standby = true;
    }
    fn wakeup(&self) {
        let mut s = self.state.lock().unwrap();
        s.standby = false;
    }
    fn reset_reason(&self) -> ResetReason {
        current_reset_reason()
    }
    fn abnormal_boot_count(&self) -> u32 {
        let diag: DiagInfo = self.storage.load_diag();
        diag.abnormal_boot_cnt
    }
}

unsafe impl<S: StorageService + Send> Send for HalPowerService<S> {}
unsafe impl<S: StorageService + Send> Sync for HalPowerService<S> {}

/// Free function mapping ESP-IDF reset reason to project's `ResetReason`.
/// Used by `HalPowerService::reset_reason()` and by `main.rs` early in boot
/// (before `Hal` is constructed) for the safety-diagnostics flow (D2).
pub fn map_idf_reset_reason(r: IdfResetReason) -> ResetReason {
    match r {
        IdfResetReason::PowerOn | IdfResetReason::DeepSleep | IdfResetReason::PowerGlitch => {
            ResetReason::PowerOn
        }
        IdfResetReason::Brownout => ResetReason::Brownout,
        IdfResetReason::Watchdog
        | IdfResetReason::InterruptWatchdog
        | IdfResetReason::TaskWatchdog => ResetReason::Wdt,
        IdfResetReason::Panic => ResetReason::Panic,
        _ => ResetReason::Unknown,
    }
}

/// Read the current boot's reset reason. Convenience wrapper for `main.rs`
/// to call before any service is constructed.
pub fn current_reset_reason() -> ResetReason {
    map_idf_reset_reason(IdfResetReason::get())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_icon_hysteresis() {
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Full,
                3.8
            ),
            BatteryIcon::Medium
        );
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Medium,
                3.7
            ),
            BatteryIcon::Medium
        );
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Medium,
                3.5
            ),
            BatteryIcon::Low
        );
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Low,
                3.3
            ),
            BatteryIcon::Critical
        );
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Critical,
                3.5
            ),
            BatteryIcon::Low
        );
    }

    #[test]
    fn voltage_icon_uses_board_profile_divider() {
        assert_eq!(BoardProfile::BAT_ADC_DIVIDER, 3);
    }
}
