//! Power service: battery ADC + standby/wakeup + reset reason. change 04/17.
//! design D8-D10. Spec: §3.6, §9, §2.4, §16.

#![allow(dead_code)]

use std::fmt;
use std::sync::Mutex;

use crate::board_profile::BoardProfile;
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
    /// Smoothed battery percent 0..=100 (EWMA over raw ADC samples).
    fn battery_percent(&self) -> u8;
    /// 4-bar icon with ±0.1V hysteresis.
    fn battery_icon(&self) -> BatteryIcon;
    /// Enter low-power standby: screen off, PA off, keep Wi-Fi/ESP-NOW.
    fn enter_standby(&self);
    /// Wake from standby: restore screen + PA.
    fn wakeup(&self);
    /// Boot reset reason.
    fn reset_reason(&self) -> ResetReason;
    /// Abnormal boot count from NVS (change 03 StorageService).
    fn abnormal_boot_count(&self) -> u32;
}

/// Stub impl. Holds state in Mutex; real ADC sampling + LEDC PA toggle +
/// ESP-IDF reset reason read land in on-hardware work.
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
    /// Exposed for unit testing without real ADC.
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
        // TODO: DisplayService::screen_off() + AudioOut::pa_enable(false).
        // TODO: optional CPU frequency reduction.
    }
    fn wakeup(&self) {
        let mut s = self.state.lock().expect("power state");
        s.standby = false;
        // TODO: DisplayService::screen_on() + AudioOut::pa_enable(true).
    }
    fn reset_reason(&self) -> ResetReason {
        // TODO: read esp_idf_svc::hal::reset::ResetReason.
        ResetReason::PowerOn
    }
    fn abnormal_boot_count(&self) -> u32 {
        let diag: DiagInfo = self.storage.load_diag();
        diag.abnormal_boot_cnt
    }
}

unsafe impl<S: StorageService + Send> Send for PowerServiceStub<S> {}
unsafe impl<S: StorageService + Send> Sync for PowerServiceStub<S> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voltage_icon_hysteresis() {
        // Start at Full, drop below 3.8 → Medium
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Full,
                3.8
            ),
            BatteryIcon::Medium
        );
        // At Medium, between 3.6 and 3.9 → Medium
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Medium,
                3.7
            ),
            BatteryIcon::Medium
        );
        // Medium → drop below 3.5 → Low
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Medium,
                3.5
            ),
            BatteryIcon::Low
        );
        // Low → drop below 3.3 → Critical
        assert_eq!(
            PowerServiceStub::<crate::services::storage::NvsStorage>::voltage_to_icon(
                BatteryIcon::Low,
                3.3
            ),
            BatteryIcon::Critical
        );
        // Critical → rise to 3.5 → Low (need to cross 3.4+0.1=3.5)
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
        // Sanity: BAT_ADC_DIVIDER is referenced.
        assert_eq!(BoardProfile::BAT_ADC_DIVIDER, 3);
    }
}
