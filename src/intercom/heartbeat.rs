//! Restore + heartbeat (change 12/17). §3.5, §5.2 cold-boot restore.
#![allow(dead_code)]
use std::fmt;

pub trait HeartbeatService: Send + Sync + fmt::Debug {
    fn tick(&self);
    fn on_heartbeat_recv(&self, src_mac: &[u8; 6]);
    fn last_seen(&self, src_mac: &[u8; 6]) -> Option<u64>;
}

pub trait RestoreService: Send + Sync + fmt::Debug {
    fn restore_last_state(&self) -> Option<u8>;
    fn save_current_state(&self, app_id: u8);
}

pub struct HeartbeatServiceStub;
pub struct RestoreServiceStub;

impl fmt::Debug for HeartbeatServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("HeartbeatServiceStub").finish() }
}
impl fmt::Debug for RestoreServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.debug_struct("RestoreServiceStub").finish() }
}

impl HeartbeatService for HeartbeatServiceStub {
    fn tick(&self) {}
    fn on_heartbeat_recv(&self, _: &[u8; 6]) {}
    fn last_seen(&self, _: &[u8; 6]) -> Option<u64> { None }
}
impl RestoreService for RestoreServiceStub {
    fn restore_last_state(&self) -> Option<u8> { None }
    fn save_current_state(&self, _: u8) {}
}
unsafe impl Send for HeartbeatServiceStub {}
unsafe impl Sync for HeartbeatServiceStub {}
unsafe impl Send for RestoreServiceStub {}
unsafe impl Sync for RestoreServiceStub {}
