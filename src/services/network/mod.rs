//! Network service: ESP-NOW packet format + add_peer + send/recv. change 06/17.
//! Spec: §3.3. design D1-D5 (see change 06 design.md).

#![allow(dead_code)]
#![allow(unused_imports)]

use std::fmt;

use crate::board_profile::BoardProfile;

/// Max ESP-NOW payload (IEEE 802.11 management frame, ~250 bytes after security header).
pub const MAX_PAYLOAD: usize = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecvEvent {
    pub src_mac: [u8; 6],
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    EspNow,
    InvalidParam,
    PeerLimit,
    NotInitialized,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::EspNow => write!(f, "espnow error"),
            NetError::InvalidParam => write!(f, "invalid param"),
            NetError::PeerLimit => write!(f, "peer limit exceeded"),
            NetError::NotInitialized => write!(f, "not initialized"),
        }
    }
}
impl std::error::Error for NetError {}

/// Network service trait. §3.3.
pub trait NetworkService: Send + Sync + fmt::Debug {
    fn init(&self, channel: u8) -> Result<(), NetError>;
    fn set_channel(&self, channel: u8) -> Result<(), NetError>;
    fn add_peer(&self, mac: &[u8; 6], lmk: Option<&[u8; 16]>) -> Result<(), NetError>;
    fn remove_peer(&self, mac: &[u8; 6]) -> Result<(), NetError>;
    fn clear_peers(&self) -> Result<(), NetError>;
    fn send(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError>;
    fn broadcast(&self, payload: &[u8]) -> Result<(), NetError>;
    /// Register a single receive callback (replaces any prior).
    fn on_recv(&self, cb: Box<dyn Fn(RecvEvent) + Send + Sync>);
    fn current_channel(&self) -> u8;
    fn peer_count(&self) -> u8;
}

/// Stub impl. Real EspNow wiring (from change 02 RadioDriver) deferred to
/// on-hardware verification.
pub struct EspNowNetworkServiceStub {
    channel: u8,
}

impl fmt::Debug for EspNowNetworkServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EspNowNetworkServiceStub")
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl EspNowNetworkServiceStub {
    pub fn new() -> Self {
        Self {
            channel: BoardProfile::DISCOVERY_CHANNEL,
        }
    }
}

impl Default for EspNowNetworkServiceStub {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkService for EspNowNetworkServiceStub {
    fn init(&self, channel: u8) -> Result<(), NetError> {
        let _ = channel;
        Ok(())
    }
    fn set_channel(&self, _channel: u8) -> Result<(), NetError> {
        Ok(())
    }
    fn add_peer(&self, _mac: &[u8; 6], _lmk: Option<&[u8; 16]>) -> Result<(), NetError> {
        Ok(())
    }
    fn remove_peer(&self, _mac: &[u8; 6]) -> Result<(), NetError> {
        Ok(())
    }
    fn clear_peers(&self) -> Result<(), NetError> {
        Ok(())
    }
    fn send(&self, _dst: &[u8; 6], _payload: &[u8]) -> Result<(), NetError> {
        Ok(())
    }
    fn broadcast(&self, _payload: &[u8]) -> Result<(), NetError> {
        Ok(())
    }
    fn on_recv(&self, _cb: Box<dyn Fn(RecvEvent) + Send + Sync>) {}
    fn current_channel(&self) -> u8 {
        self.channel
    }
    fn peer_count(&self) -> u8 {
        0
    }
}

unsafe impl Send for EspNowNetworkServiceStub {}
unsafe impl Sync for EspNowNetworkServiceStub {}
