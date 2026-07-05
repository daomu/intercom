//! Settings app (change 07/17). §3.7 settings panel.
#![allow(dead_code)]
use std::fmt;

use crate::services::storage::Settings;

pub trait SettingsApp: Send + Sync + fmt::Debug {
    fn load(&self) -> Settings;
    fn save(&self, s: &Settings);
    fn reset(&self);
}

pub struct SettingsAppStub;
impl fmt::Debug for SettingsAppStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("SettingsAppStub").finish() }
}
impl SettingsApp for SettingsAppStub {
    fn load(&self) -> Settings { Settings::default() }
    fn save(&self, _: &Settings) {}
    fn reset(&self) {}
}
unsafe impl Send for SettingsAppStub {}
unsafe impl Sync for SettingsAppStub {}
