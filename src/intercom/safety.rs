//! Safety + diagnostics (change 16). Spec: §3.5, §7.4-7.5, §8, §17.
//!
//! Pure-logic safety evaluation:
//! - `safety_action(reset_reason, abnormal_boot_cnt, safe_boot_flag) -> SafetyAction`
//! - `should_inc_abnormal(reset_reason) -> bool`
//! - `schema_compatible(remote_ver, local_ver) -> bool`
//! Plus a `SafetyStateMachine` that tracks state across boot cycles.

#![allow(dead_code)]

use std::fmt;

use crate::services::power::ResetReason;
use crate::services::storage::{SchemaAction, StorageService};

/// Threshold for entering safe-boot mode (D2).
pub const ABNORMAL_BOOT_THRESHOLD: u32 = 3;

/// Outcomes of the safety evaluation at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyAction {
    /// Normal boot — proceed with full system init.
    None,
    /// Increment abnormal_boot_cnt but continue normal boot.
    Warn,
    /// Enter safe-boot mode: skip IntercomService/NetworkService, only Settings.
    SafeBoot,
    /// Force standby (low battery / thermal limit — caller-driven).
    ForceStandby,
}

/// Decide whether to increment abnormal_boot_cnt based on reset reason (D2).
/// Only non-PowerOn resets increment the counter.
pub fn should_inc_abnormal(reset: ResetReason) -> bool {
    !matches!(reset, ResetReason::PowerOn)
}

/// Decide the boot-time safety action. `safe_boot_flag` is the persisted
/// flag from `DiagInfo` — if it's true, we stay in safe-boot mode until
/// the user performs a factory reset.
pub fn safety_action(
    reset: ResetReason,
    abnormal_boot_cnt: u32,
    safe_boot_flag: bool,
) -> SafetyAction {
    if safe_boot_flag {
        return SafetyAction::SafeBoot;
    }
    let inc = should_inc_abnormal(reset);
    let cnt_after = if inc { abnormal_boot_cnt + 1 } else { abnormal_boot_cnt };
    if cnt_after >= ABNORMAL_BOOT_THRESHOLD {
        SafetyAction::SafeBoot
    } else if inc {
        SafetyAction::Warn
    } else {
        SafetyAction::None
    }
}

/// Check whether a remote schema_ver is compatible with the local one.
/// Per PRD §5.4 / D6, only exact equality is accepted in phase 1.
pub fn schema_compatible(remote_ver: u16, local_ver: u16) -> bool {
    remote_ver == local_ver
}

/// Pairing JOIN_ACK reason code for a failed schema check.
pub const SCHEMA_MISMATCH_REASON: u8 = 2;

// ---- SafetyStateMachine: drives boot-time side effects -------------------

/// States the safety machine can be in across the boot lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyState {
    /// Initial state — no decision yet.
    Pending,
    /// Normal boot proceeding.
    Normal,
    /// Safe-boot mode entered.
    SafeBoot,
    /// Standby forced by caller (e.g. low battery).
    Standby,
}

pub struct SafetyStateMachine {
    state: SafetyState,
    safe_boot_flag: bool,
    abnormal_boot_cnt: u32,
}

impl fmt::Debug for SafetyStateMachine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SafetyStateMachine")
            .field("state", &self.state)
            .field("safe_boot_flag", &self.safe_boot_flag)
            .field("abnormal_boot_cnt", &self.abnormal_boot_cnt)
            .finish()
    }
}

impl SafetyStateMachine {
    pub fn new() -> Self {
        Self {
            state: SafetyState::Pending,
            safe_boot_flag: false,
            abnormal_boot_cnt: 0,
        }
    }

    pub fn state(&self) -> SafetyState {
        self.state
    }

    pub fn abnormal_boot_cnt(&self) -> u32 {
        self.abnormal_boot_cnt
    }

    pub fn safe_boot_flag(&self) -> bool {
        self.safe_boot_flag
    }

    /// Evaluate at boot. Reads `diag` (loaded from NVS by caller), the
    /// current `reset_reason`, and the local `schema_ver`. Returns the
    /// action to take; mutates internal state.
    pub fn evaluate(
        &mut self,
        reset: ResetReason,
        diag_abnormal_cnt: u32,
        diag_safe_boot_flag: bool,
    ) -> SafetyAction {
        self.abnormal_boot_cnt = diag_abnormal_cnt;
        self.safe_boot_flag = diag_safe_boot_flag;
        let action = safety_action(reset, diag_abnormal_cnt, diag_safe_boot_flag);
        self.state = match action {
            SafetyAction::None | SafetyAction::Warn => SafetyState::Normal,
            SafetyAction::SafeBoot => SafetyState::SafeBoot,
            SafetyAction::ForceStandby => SafetyState::Standby,
        };
        action
    }

    /// Called after a successful full boot (D4): clears abnormal_boot_cnt
    /// and safe_boot_flag.
    pub fn on_boot_completed(&mut self) {
        self.abnormal_boot_cnt = 0;
        self.safe_boot_flag = false;
    }

    /// Called when user performs "clear group" inside safe-boot Settings.
    pub fn on_clear_group(&mut self) {
        // Clearing the group does NOT clear safe_boot_flag (D3).
        // User must factory-reset to exit safe-boot.
    }

    /// Called on factory reset — clears all safety state.
    pub fn on_factory_reset(&mut self) {
        self.abnormal_boot_cnt = 0;
        self.safe_boot_flag = false;
        self.state = SafetyState::Pending;
    }
}

impl Default for SafetyStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

// ---- Boot orchestration plan -------------------------------------------

/// One-time boot plan computed from the safety evaluation. The caller
/// (main.rs / Launcher) executes these steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPlan {
    pub action: SafetyAction,
    /// Whether to call `StorageService::inc_abnormal_boot()`.
    pub should_inc: bool,
    /// Whether to start IntercomService + NetworkService.
    pub start_intercom: bool,
    /// Whether to start Settings App only (safe-boot mode).
    pub settings_only: bool,
    /// Whether to display the "safe mode" indicator.
    pub show_safe_mode_indicator: bool,
}

impl BootPlan {
    /// Compute a boot plan from the safety action.
    pub fn from_action(action: SafetyAction, reset: ResetReason) -> Self {
        let should_inc = should_inc_abnormal(reset);
        match action {
            SafetyAction::None => BootPlan {
                action,
                should_inc,
                start_intercom: true,
                settings_only: false,
                show_safe_mode_indicator: false,
            },
            SafetyAction::Warn => BootPlan {
                action,
                should_inc,
                start_intercom: true,
                settings_only: false,
                show_safe_mode_indicator: false,
            },
            SafetyAction::SafeBoot => BootPlan {
                action,
                should_inc: false, // already in safe-boot; don't double-inc
                start_intercom: false,
                settings_only: true,
                show_safe_mode_indicator: true,
            },
            SafetyAction::ForceStandby => BootPlan {
                action,
                should_inc,
                start_intercom: false,
                settings_only: false,
                show_safe_mode_indicator: false,
            },
        }
    }
}

// ---- Data-corruption recovery helpers (D7) ------------------------------

/// What to do when an NVS namespace read fails (D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorruptRecovery {
    /// Reset sys namespace to defaults.
    ResetSettings,
    /// Clear group namespace (incl. keys).
    ClearGroup,
    /// Use zero defaults for diag namespace.
    DiagDefaults,
}

pub fn recovery_for_namespace(namespace: &str) -> CorruptRecovery {
    match namespace {
        "sys" => CorruptRecovery::ResetSettings,
        "group" => CorruptRecovery::ClearGroup,
        _ => CorruptRecovery::DiagDefaults,
    }
}

// ---- Trait shim ---------------------------------------------------------

pub trait SafetyService: Send + Sync + fmt::Debug {
    fn evaluate(&self, reset: ResetReason, abnormal_boot_cnt: u32) -> SafetyAction;
    fn safe_boot_flag(&self) -> bool;
    fn set_safe_boot_flag(&self, v: bool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poweron_does_not_increment() {
        assert!(!should_inc_abnormal(ResetReason::PowerOn));
    }

    #[test]
    fn abnormal_resets_increment() {
        assert!(should_inc_abnormal(ResetReason::Brownout));
        assert!(should_inc_abnormal(ResetReason::Wdt));
        assert!(should_inc_abnormal(ResetReason::Panic));
        assert!(should_inc_abnormal(ResetReason::Unknown));
    }

    #[test]
    fn safe_boot_flag_forces_safeboot() {
        let a = safety_action(ResetReason::PowerOn, 0, true);
        assert_eq!(a, SafetyAction::SafeBoot);
    }

    #[test]
    fn threshold_triggers_safeboot() {
        // cnt=2 + inc → cnt_after=3 → SafeBoot
        let a = safety_action(ResetReason::Wdt, 2, false);
        assert_eq!(a, SafetyAction::SafeBoot);
    }

    #[test]
    fn below_threshold_warns() {
        let a = safety_action(ResetReason::Wdt, 0, false);
        assert_eq!(a, SafetyAction::Warn);
        let a = safety_action(ResetReason::Wdt, 1, false);
        assert_eq!(a, SafetyAction::Warn);
    }

    #[test]
    fn poweron_below_threshold_is_none() {
        let a = safety_action(ResetReason::PowerOn, 1, false);
        assert_eq!(a, SafetyAction::None);
    }

    #[test]
    fn schema_compatible_exact_match_only() {
        assert!(schema_compatible(1, 1));
        assert!(!schema_compatible(1, 2));
        assert!(!schema_compatible(2, 1));
    }

    #[test]
    fn state_machine_transitions() {
        let mut m = SafetyStateMachine::new();
        // First abnormal boot: cnt=0 → Warn (cnt_after=1)
        let a = m.evaluate(ResetReason::Wdt, 0, false);
        assert_eq!(a, SafetyAction::Warn);
        assert_eq!(m.state(), SafetyState::Normal);

        // Second boot with cnt=1 → Warn (cnt_after=2)
        let a = m.evaluate(ResetReason::Wdt, 1, false);
        assert_eq!(a, SafetyAction::Warn);

        // Third boot with cnt=2 → SafeBoot (cnt_after=3)
        let a = m.evaluate(ResetReason::Wdt, 2, false);
        assert_eq!(a, SafetyAction::SafeBoot);
        assert_eq!(m.state(), SafetyState::SafeBoot);

        // Once in safe-boot, the flag is set
        let a = m.evaluate(ResetReason::PowerOn, 5, true);
        assert_eq!(a, SafetyAction::SafeBoot);
    }

    #[test]
    fn boot_plan_normal() {
        let p = BootPlan::from_action(SafetyAction::None, ResetReason::PowerOn);
        assert!(p.start_intercom);
        assert!(!p.settings_only);
        assert!(!p.show_safe_mode_indicator);
        assert!(!p.should_inc);
    }

    #[test]
    fn boot_plan_safeboot() {
        let p = BootPlan::from_action(SafetyAction::SafeBoot, ResetReason::Wdt);
        assert!(!p.start_intercom);
        assert!(p.settings_only);
        assert!(p.show_safe_mode_indicator);
        // Don't double-inc in safe-boot
        assert!(!p.should_inc);
    }

    #[test]
    fn boot_plan_warn_increments() {
        let p = BootPlan::from_action(SafetyAction::Warn, ResetReason::Wdt);
        assert!(p.should_inc);
        assert!(p.start_intercom);
    }

    #[test]
    fn recovery_for_namespace_mapping() {
        assert_eq!(recovery_for_namespace("sys"), CorruptRecovery::ResetSettings);
        assert_eq!(recovery_for_namespace("group"), CorruptRecovery::ClearGroup);
        assert_eq!(recovery_for_namespace("diag"), CorruptRecovery::DiagDefaults);
        assert_eq!(recovery_for_namespace("unknown"), CorruptRecovery::DiagDefaults);
    }

    #[test]
    fn on_boot_completed_clears_state() {
        let mut m = SafetyStateMachine::new();
        m.evaluate(ResetReason::Wdt, 2, false);
        m.on_boot_completed();
        assert_eq!(m.abnormal_boot_cnt(), 0);
        assert!(!m.safe_boot_flag());
    }

    #[test]
    fn factory_reset_clears_state() {
        let mut m = SafetyStateMachine::new();
        m.evaluate(ResetReason::Wdt, 5, true);
        m.on_factory_reset();
        assert_eq!(m.abnormal_boot_cnt(), 0);
        assert!(!m.safe_boot_flag());
        assert_eq!(m.state(), SafetyState::Pending);
    }
}
