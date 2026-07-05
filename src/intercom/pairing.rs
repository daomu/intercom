//! Pairing three-phase state machine (change 09/17). §3.5, §8.
//! Stub: PairingService trait + PairingState enum.

#![allow(dead_code)]

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingState {
    Idle,
    Discovery,
    ProbeReceived,
    JoinRequesting,
    JoinAcknowledged,
    Failed,
}

pub trait PairingService: Send + Sync + fmt::Debug {
    fn start_discovery(&self) -> Result<(), PairingError>;
    fn on_packet(&self, state: &mut PairingState, p: &crate::intercom::packet::Packet);
    fn current_state(&self) -> PairingState;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    NotIdle,
    NoPeer,
    Timeout,
    Crypto,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingError::NotIdle => write!(f, "not idle"),
            PairingError::NoPeer => write!(f, "no peer"),
            PairingError::Timeout => write!(f, "timeout"),
            PairingError::Crypto => write!(f, "crypto"),
        }
    }
}
impl std::error::Error for PairingError {}

pub struct PairingServiceStub;
impl fmt::Debug for PairingServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingServiceStub").finish()
    }
}
impl PairingService for PairingServiceStub {
    fn start_discovery(&self) -> Result<(), PairingError> { Ok(()) }
    fn on_packet(&self, _state: &mut PairingState, _p: &crate::intercom::packet::Packet) {}
    fn current_state(&self) -> PairingState { PairingState::Idle }
}
unsafe impl Send for PairingServiceStub {}
unsafe impl Sync for PairingServiceStub {}
