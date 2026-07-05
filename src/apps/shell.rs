//! App shell / launcher (change 07/17). §3.7 launcher.
#![allow(dead_code)]
use std::fmt;

pub trait AppShell: Send + Sync + fmt::Debug {
    fn launch(&self, app_id: u8);
    fn current_app(&self) -> u8;
    fn back(&self);
}

pub struct AppShellStub;
impl fmt::Debug for AppShellStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("AppShellStub").finish() }
}
impl AppShell for AppShellStub {
    fn launch(&self, _: u8) {}
    fn current_app(&self) -> u8 { 0 }
    fn back(&self) {}
}
unsafe impl Send for AppShellStub {}
unsafe impl Sync for AppShellStub {}
