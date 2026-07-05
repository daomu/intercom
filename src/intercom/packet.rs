//! Intercom packet format (change 08/17). §3.5, §6 packet layout.
//! Stub: Packet struct + encode/decode signatures.

#![allow(dead_code)]

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    PairProbe = 0,
    PairJoinReq = 1,
    PairJoinAck = 2,
    PairLeave = 3,
    Voice = 4,
    Heartbeat = 5,
    ModeSwitch = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub ptype: PacketType,
    pub seq: u16,
    pub src_id: u16,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        // TODO: design §6 wire format (header + payload + MIC).
        Vec::new()
    }
    pub fn decode(_bytes: &[u8]) -> Result<Self, PacketError> {
        Err(PacketError::Truncated)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    Truncated,
    BadMagic,
    BadMic,
    UnknownType,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketError::Truncated => write!(f, "truncated"),
            PacketError::BadMagic => write!(f, "bad magic"),
            PacketError::BadMic => write!(f, "bad mic"),
            PacketError::UnknownType => write!(f, "unknown type"),
        }
    }
}
impl std::error::Error for PacketError {}
