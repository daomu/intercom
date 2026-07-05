//! Intercom packet wire format. Spec: §4, §5. change 08.
//!
//! 9 packet types share an 8-byte header (magic=0xC6, ver, type, flags,
//! seq BE u16, len BE u16). All encode_* functions write into a caller-provided
//! &mut [u8]; all decode_* functions return views into the caller's buffer,
//! avoiding heap allocation on the realtime audio path (PRD §16.9).

#![allow(dead_code)]

use std::fmt;

use crate::services::network::NetError;

// ---- Header constants -----------------------------------------------------

pub const MAGIC: u8 = 0xC6;
pub const SCHEMA_VER: u8 = 1;
pub const HEADER_LEN: usize = 8;
const OFF_MAGIC: usize = 0;
const OFF_VER: usize = 1;
const OFF_TYPE: usize = 2;
const OFF_FLAGS: usize = 3;
const OFF_SEQ: usize = 4;
const OFF_LEN: usize = 6;

// ---- PacketType -----------------------------------------------------------

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    Voice = 0x01,
    Heartbeat = 0x02,
    TalkState = 0x03,
    CtrlBusyReady = 0x04,
    PairBeaconHost = 0x10,
    PairJoinReq = 0x11,
    PairJoinAck = 0x12,
    DirectoryBroadcast = 0x20,
    ChannelSwitchAck = 0x30,
}

impl PacketType {
    pub fn from_u8(v: u8) -> Result<Self, PacketError> {
        match v {
            0x01 => Ok(PacketType::Voice),
            0x02 => Ok(PacketType::Heartbeat),
            0x03 => Ok(PacketType::TalkState),
            0x04 => Ok(PacketType::CtrlBusyReady),
            0x10 => Ok(PacketType::PairBeaconHost),
            0x11 => Ok(PacketType::PairJoinReq),
            0x12 => Ok(PacketType::PairJoinAck),
            0x20 => Ok(PacketType::DirectoryBroadcast),
            0x30 => Ok(PacketType::ChannelSwitchAck),
            _ => Err(PacketError::BadType),
        }
    }
}

// ---- PacketError ----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketError {
    BadMagic,
    BadVersion,
    Truncated,
    BadType,
    PayloadTooLarge,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PacketError::BadMagic => write!(f, "bad magic"),
            PacketError::BadVersion => write!(f, "bad version"),
            PacketError::Truncated => write!(f, "truncated"),
            PacketError::BadType => write!(f, "bad type"),
            PacketError::PayloadTooLarge => write!(f, "payload too large"),
        }
    }
}
impl std::error::Error for PacketError {}

// ---- PacketHeader ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketHeader {
    pub ver: u8,
    pub ptype: PacketType,
    pub flags: u8,
    pub seq: u16,
    pub len: u16,
}

impl PacketHeader {
    /// Parse 8-byte header. Validates magic only (D19: ver deferred to caller).
    /// Returns header and a view into the payload portion of `buf`.
    pub fn parse(buf: &[u8]) -> Result<(Self, &[u8]), PacketError> {
        if buf.len() < HEADER_LEN {
            return Err(PacketError::Truncated);
        }
        if buf[OFF_MAGIC] != MAGIC {
            return Err(PacketError::BadMagic);
        }
        let ptype = PacketType::from_u8(buf[OFF_TYPE])?;
        let seq = u16::from_be_bytes([buf[OFF_SEQ], buf[OFF_SEQ + 1]]);
        let len = u16::from_be_bytes([buf[OFF_LEN], buf[OFF_LEN + 1]]);
        let header = PacketHeader {
            ver: buf[OFF_VER],
            ptype,
            flags: buf[OFF_FLAGS],
            seq,
            len,
        };
        let payload = &buf[HEADER_LEN..];
        Ok((header, payload))
    }

    /// Write 8-byte header into `buf`. Validates that `self.len` fits in u16
    /// and `buf` has room for HEADER_LEN bytes.
    pub fn encode(&self, buf: &mut [u8]) -> Result<(), PacketError> {
        // len is already u16, but guard against callers passing a length that
        // would overflow when combined with payload writes.
        if buf.len() < HEADER_LEN {
            return Err(PacketError::Truncated);
        }
        buf[OFF_MAGIC] = MAGIC;
        buf[OFF_VER] = self.ver;
        buf[OFF_TYPE] = self.ptype as u8;
        buf[OFF_FLAGS] = self.flags;
        buf[OFF_SEQ..OFF_SEQ + 2].copy_from_slice(&self.seq.to_be_bytes());
        buf[OFF_LEN..OFF_LEN + 2].copy_from_slice(&self.len.to_be_bytes());
        Ok(())
    }
}

// Helper: ensure buf has room for header + N payload bytes.
fn ensure_capacity(buf: &[u8], payload_len: usize) -> Result<(), PacketError> {
    if buf.len() < HEADER_LEN + payload_len {
        Err(PacketError::Truncated)
    } else {
        Ok(())
    }
}

// Helper: ensure payload slice has at least `n` bytes.
fn require_len(payload: &[u8], n: usize) -> Result<(), PacketError> {
    if payload.len() < n {
        Err(PacketError::Truncated)
    } else {
        Ok(())
    }
}

// ---- VOICE (§4.2) ---------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoicePayload<'a> {
    pub sender_id: u8,
    pub effect: u8,
    pub opus_payload: &'a [u8],
}

/// payload layout: [sender_id(1), effect(1), opus_payload(..)]
pub fn encode_voice(
    header: &PacketHeader,
    payload: &VoicePayload<'_>,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    let plen = 2 + payload.opus_payload.len();
    ensure_capacity(buf, plen)?;
    let mut h = *header;
    h.ptype = PacketType::Voice;
    h.len = plen as u16;
    h.encode(buf)?;
    buf[HEADER_LEN] = payload.sender_id;
    buf[HEADER_LEN + 1] = payload.effect;
    buf[HEADER_LEN + 2..HEADER_LEN + plen].copy_from_slice(payload.opus_payload);
    Ok(HEADER_LEN + plen)
}

pub fn decode_voice(buf: &[u8]) -> Result<(PacketHeader, VoicePayload<'_>), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 2)?;
    let opus = &payload[2..];
    Ok((
        header,
        VoicePayload {
            sender_id: payload[0],
            effect: payload[1],
            opus_payload: opus,
        },
    ))
}

// ---- HEARTBEAT (§4.3) -----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatPayload {
    pub sender_id: u8,
    pub state: u8,
    pub mode: u8,
}

pub fn encode_heartbeat(
    header: &PacketHeader,
    payload: &HeartbeatPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 3;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::Heartbeat;
    h.len = PLEN as u16;
    h.encode(buf)?;
    buf[HEADER_LEN] = payload.sender_id;
    buf[HEADER_LEN + 1] = payload.state;
    buf[HEADER_LEN + 2] = payload.mode;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_heartbeat(buf: &[u8]) -> Result<(PacketHeader, HeartbeatPayload), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 3)?;
    Ok((
        header,
        HeartbeatPayload {
            sender_id: payload[0],
            state: payload[1],
            mode: payload[2],
        },
    ))
}

// ---- TALK_STATE (§4.4) ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TalkStatePayload {
    pub sender_id: u8,
    pub action: u8,
}

pub fn encode_talk_state(
    header: &PacketHeader,
    payload: &TalkStatePayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 2;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::TalkState;
    h.len = PLEN as u16;
    h.encode(buf)?;
    buf[HEADER_LEN] = payload.sender_id;
    buf[HEADER_LEN + 1] = payload.action;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_talk_state(buf: &[u8]) -> Result<(PacketHeader, TalkStatePayload), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 2)?;
    Ok((
        header,
        TalkStatePayload {
            sender_id: payload[0],
            action: payload[1],
        },
    ))
}

// ---- CTRL_BUSY_READY (§4.5) ----------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtrlBusyReadyPayload {
    pub dst_id: u8,
    pub code: u8,
}

pub fn encode_ctrl_busy_ready(
    header: &PacketHeader,
    payload: &CtrlBusyReadyPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 2;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::CtrlBusyReady;
    h.len = PLEN as u16;
    h.encode(buf)?;
    buf[HEADER_LEN] = payload.dst_id;
    buf[HEADER_LEN + 1] = payload.code;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_ctrl_busy_ready(
    buf: &[u8],
) -> Result<(PacketHeader, CtrlBusyReadyPayload), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 2)?;
    Ok((
        header,
        CtrlBusyReadyPayload {
            dst_id: payload[0],
            code: payload[1],
        },
    ))
}

// ---- PAIR_BEACON_HOST (§5.2) ---------------------------------------------

/// payload = host_mac(6) + host_pub_key(32) + mode(1) + cur_members(1)
///         + max_members(1) + joinable(1) = 42 bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairBeaconHostPayload {
    pub host_mac: [u8; 6],
    pub host_pub_key: [u8; 32],
    pub mode: u8,
    pub cur_members: u8,
    pub max_members: u8,
    pub joinable: u8,
}

pub fn encode_pair_beacon_host(
    header: &PacketHeader,
    payload: &PairBeaconHostPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 42;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::PairBeaconHost;
    h.len = PLEN as u16;
    h.encode(buf)?;
    let off = HEADER_LEN;
    buf[off..off + 6].copy_from_slice(&payload.host_mac);
    buf[off + 6..off + 38].copy_from_slice(&payload.host_pub_key);
    buf[off + 38] = payload.mode;
    buf[off + 39] = payload.cur_members;
    buf[off + 40] = payload.max_members;
    buf[off + 41] = payload.joinable;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_pair_beacon_host(
    buf: &[u8],
) -> Result<(PacketHeader, PairBeaconHostPayload), PacketError> {
    const PLEN: usize = 42;
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, PLEN)?;
    let mut host_mac = [0u8; 6];
    host_mac.copy_from_slice(&payload[0..6]);
    let mut host_pub_key = [0u8; 32];
    host_pub_key.copy_from_slice(&payload[6..38]);
    Ok((
        header,
        PairBeaconHostPayload {
            host_mac,
            host_pub_key,
            mode: payload[38],
            cur_members: payload[39],
            max_members: payload[40],
            joinable: payload[41],
        },
    ))
}

// ---- PAIR_JOIN_REQ (§5.3) ------------------------------------------------

/// payload = join_mac(6) + join_pub_key(32) + host_mac(6) = 44 bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairJoinReqPayload {
    pub join_mac: [u8; 6],
    pub join_pub_key: [u8; 32],
    pub host_mac: [u8; 6],
}

pub fn encode_pair_join_req(
    header: &PacketHeader,
    payload: &PairJoinReqPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 44;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::PairJoinReq;
    h.len = PLEN as u16;
    h.encode(buf)?;
    let off = HEADER_LEN;
    buf[off..off + 6].copy_from_slice(&payload.join_mac);
    buf[off + 6..off + 38].copy_from_slice(&payload.join_pub_key);
    buf[off + 38..off + 44].copy_from_slice(&payload.host_mac);
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_pair_join_req(
    buf: &[u8],
) -> Result<(PacketHeader, PairJoinReqPayload), PacketError> {
    const PLEN: usize = 44;
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, PLEN)?;
    let mut join_mac = [0u8; 6];
    join_mac.copy_from_slice(&payload[0..6]);
    let mut join_pub_key = [0u8; 32];
    join_pub_key.copy_from_slice(&payload[6..38]);
    let mut host_mac = [0u8; 6];
    host_mac.copy_from_slice(&payload[38..44]);
    Ok((
        header,
        PairJoinReqPayload {
            join_mac,
            join_pub_key,
            host_mac,
        },
    ))
}

// ---- PAIR_JOIN_ACK (§5.4) ------------------------------------------------

/// payload = host_mac(6) + host_pub_key(32) + join_mac(6) + accepted(1) + reason(1) = 46 bytes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairJoinAckPayload {
    pub host_mac: [u8; 6],
    pub host_pub_key: [u8; 32],
    pub join_mac: [u8; 6],
    pub accepted: u8,
    pub reason: u8,
}

pub fn encode_pair_join_ack(
    header: &PacketHeader,
    payload: &PairJoinAckPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 46;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::PairJoinAck;
    h.len = PLEN as u16;
    h.encode(buf)?;
    let off = HEADER_LEN;
    buf[off..off + 6].copy_from_slice(&payload.host_mac);
    buf[off + 6..off + 38].copy_from_slice(&payload.host_pub_key);
    buf[off + 38..off + 44].copy_from_slice(&payload.join_mac);
    buf[off + 44] = payload.accepted;
    buf[off + 45] = payload.reason;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_pair_join_ack(
    buf: &[u8],
) -> Result<(PacketHeader, PairJoinAckPayload), PacketError> {
    const PLEN: usize = 46;
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, PLEN)?;
    let mut host_mac = [0u8; 6];
    host_mac.copy_from_slice(&payload[0..6]);
    let mut host_pub_key = [0u8; 32];
    host_pub_key.copy_from_slice(&payload[6..38]);
    let mut join_mac = [0u8; 6];
    join_mac.copy_from_slice(&payload[38..44]);
    Ok((
        header,
        PairJoinAckPayload {
            host_mac,
            host_pub_key,
            join_mac,
            accepted: payload[44],
            reason: payload[45],
        },
    ))
}

// ---- DIRECTORY_BROADCAST (§5.5) ------------------------------------------

/// payload = member_count(1) + mode(1) + target_channel(1)
///         + switch_offset(BE u16, 2) + entries(member_count × 38)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryBroadcastPayload<'a> {
    pub member_count: u8,
    pub mode: u8,
    pub target_channel: u8,
    pub switch_offset: u16,
    pub entries: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry<'a> {
    pub mac: &'a [u8; 6],
    pub pub_key: &'a [u8; 32],
}

impl<'a> DirectoryBroadcastPayload<'a> {
    pub fn entry_at(&self, i: usize) -> Result<DirectoryEntry<'a>, PacketError> {
        if i >= self.member_count as usize {
            return Err(PacketError::Truncated);
        }
        let off = i * 38;
        if self.entries.len() < off + 38 {
            return Err(PacketError::Truncated);
        }
        // SAFETY: slice of length 6/32 backed by self.entries which is 'a.
        let mac: &[u8; 6] = self.entries[off..off + 6].try_into().unwrap();
        let pub_key: &[u8; 32] = self.entries[off + 6..off + 38].try_into().unwrap();
        Ok(DirectoryEntry { mac, pub_key })
    }
}

pub fn encode_directory_broadcast(
    header: &PacketHeader,
    payload: &DirectoryBroadcastPayload<'_>,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    let entries_len = payload.entries.len();
    let expected = (payload.member_count as usize) * 38;
    if entries_len < expected {
        return Err(PacketError::Truncated);
    }
    let plen = 5 + expected;
    ensure_capacity(buf, plen)?;
    let mut h = *header;
    h.ptype = PacketType::DirectoryBroadcast;
    h.len = plen as u16;
    h.encode(buf)?;
    let off = HEADER_LEN;
    buf[off] = payload.member_count;
    buf[off + 1] = payload.mode;
    buf[off + 2] = payload.target_channel;
    buf[off + 3..off + 5].copy_from_slice(&payload.switch_offset.to_be_bytes());
    buf[off + 5..off + 5 + expected].copy_from_slice(&payload.entries[..expected]);
    Ok(HEADER_LEN + plen)
}

pub fn decode_directory_broadcast<'a>(
    buf: &'a [u8],
) -> Result<(PacketHeader, DirectoryBroadcastPayload<'a>), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 5)?;
    let member_count = payload[0];
    let mode = payload[1];
    let target_channel = payload[2];
    let switch_offset = u16::from_be_bytes([payload[3], payload[4]]);
    let entries_len = member_count as usize * 38;
    require_len(payload, 5 + entries_len)?;
    let entries = &payload[5..5 + entries_len];
    Ok((
        header,
        DirectoryBroadcastPayload {
            member_count,
            mode,
            target_channel,
            switch_offset,
            entries,
        },
    ))
}

// ---- CHANNEL_SWITCH_ACK (§5.6) -------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSwitchAckPayload {
    pub sender_id: u8,
    pub status: u8,
}

pub fn encode_channel_switch_ack(
    header: &PacketHeader,
    payload: &ChannelSwitchAckPayload,
    buf: &mut [u8],
) -> Result<usize, PacketError> {
    const PLEN: usize = 2;
    ensure_capacity(buf, PLEN)?;
    let mut h = *header;
    h.ptype = PacketType::ChannelSwitchAck;
    h.len = PLEN as u16;
    h.encode(buf)?;
    buf[HEADER_LEN] = payload.sender_id;
    buf[HEADER_LEN + 1] = payload.status;
    Ok(HEADER_LEN + PLEN)
}

pub fn decode_channel_switch_ack(
    buf: &[u8],
) -> Result<(PacketHeader, ChannelSwitchAckPayload), PacketError> {
    let (header, payload) = PacketHeader::parse(buf)?;
    require_len(payload, 2)?;
    Ok((
        header,
        ChannelSwitchAckPayload {
            sender_id: payload[0],
            status: payload[1],
        },
    ))
}

// ---- Net alias re-export (for state.rs convenience) ----------------------

// NetError is re-exported here so callers of packet can also reach it
// without going to services::network directly.
pub use crate::services::network::NetError as Net;

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn base_header() -> PacketHeader {
        PacketHeader {
            ver: SCHEMA_VER,
            ptype: PacketType::Voice,
            flags: 0,
            seq: 0,
            len: 0,
        }
    }

    #[test]
    fn header_encode_be_byteorder() {
        let h = PacketHeader {
            ver: 1,
            ptype: PacketType::Voice,
            flags: 0,
            seq: 0x1234,
            len: 0x00FF,
        };
        let mut buf = [0u8; 8];
        h.encode(&mut buf).unwrap();
        assert_eq!(buf, [0xC6, 0x01, 0x01, 0x00, 0x12, 0x34, 0x00, 0xFF]);
    }

    #[test]
    fn header_parse_bad_magic() {
        let buf = [0x00u8; 8];
        assert_eq!(
            PacketHeader::parse(&buf).unwrap_err(),
            PacketError::BadMagic
        );
    }

    #[test]
    fn header_parse_truncated() {
        let buf = [0xC6u8; 4];
        assert_eq!(
            PacketHeader::parse(&buf).unwrap_err(),
            PacketError::Truncated
        );
    }

    #[test]
    fn header_parse_passes_ver_through() {
        let mut buf = [0u8; 8];
        buf[0] = 0xC6;
        buf[1] = 0x99; // mismatched ver — parse must NOT reject
        let (h, _) = PacketHeader::parse(&buf).unwrap();
        assert_eq!(h.ver, 0x99);
    }

    #[test]
    fn packettype_from_u8_known_and_unknown() {
        assert_eq!(PacketType::from_u8(0x10).unwrap(), PacketType::PairBeaconHost);
        assert_eq!(PacketType::from_u8(0xFF).unwrap_err(), PacketError::BadType);
    }

    #[test]
    fn voice_roundtrip() {
        let mut buf = [0u8; 64];
        let p = VoicePayload { sender_id: 2, effect: 1, opus_payload: &[0xDE, 0xAD] };
        let n = encode_voice(&base_header(), &p, &mut buf).unwrap();
        let (h, dec) = decode_voice(&buf[..n]).unwrap();
        assert_eq!(h.ptype, PacketType::Voice);
        assert_eq!(h.len, 4);
        assert_eq!(dec.sender_id, 2);
        assert_eq!(dec.effect, 1);
        assert_eq!(dec.opus_payload, &[0xDE, 0xAD]);
        // view points into caller buf
        let ptr = dec.opus_payload.as_ptr() as usize;
        let base = buf.as_ptr() as usize;
        assert!(ptr >= base && ptr < base + buf.len());
    }

    #[test]
    fn heartbeat_roundtrip() {
        let mut buf = [0u8; 16];
        let p = HeartbeatPayload { sender_id: 1, state: 1, mode: 0 };
        let n = encode_heartbeat(&base_header(), &p, &mut buf).unwrap();
        let (_, dec) = decode_heartbeat(&buf[..n]).unwrap();
        assert_eq!((dec.sender_id, dec.state, dec.mode), (1, 1, 0));
    }

    #[test]
    fn talk_state_roundtrip() {
        let mut buf = [0u8; 16];
        let p = TalkStatePayload { sender_id: 3, action: 1 };
        let n = encode_talk_state(&base_header(), &p, &mut buf).unwrap();
        let (_, dec) = decode_talk_state(&buf[..n]).unwrap();
        assert_eq!((dec.sender_id, dec.action), (3, 1));
    }

    #[test]
    fn ctrl_busy_ready_roundtrip() {
        let mut buf = [0u8; 16];
        let p = CtrlBusyReadyPayload { dst_id: 0, code: 2 };
        let n = encode_ctrl_busy_ready(&base_header(), &p, &mut buf).unwrap();
        let (_, dec) = decode_ctrl_busy_ready(&buf[..n]).unwrap();
        assert_eq!((dec.dst_id, dec.code), (0, 2));
    }

    #[test]
    fn pair_beacon_host_roundtrip() {
        let mut buf = [0u8; 64];
        let p = PairBeaconHostPayload {
            host_mac: [1, 2, 3, 4, 5, 6],
            host_pub_key: [0xAA; 32],
            mode: 0,
            cur_members: 1,
            max_members: 4,
            joinable: 1,
        };
        let n = encode_pair_beacon_host(&base_header(), &p, &mut buf).unwrap();
        assert_eq!(n, HEADER_LEN + 42);
        let (_, dec) = decode_pair_beacon_host(&buf[..n]).unwrap();
        assert_eq!(dec.host_mac, p.host_mac);
        assert_eq!(dec.host_pub_key, p.host_pub_key);
        assert_eq!((dec.mode, dec.cur_members, dec.max_members, dec.joinable), (0, 1, 4, 1));
    }

    #[test]
    fn pair_join_req_roundtrip() {
        let mut buf = [0u8; 64];
        let p = PairJoinReqPayload {
            join_mac: [0xA; 6],
            join_pub_key: [0xBB; 32],
            host_mac: [0xC; 6],
        };
        let n = encode_pair_join_req(&base_header(), &p, &mut buf).unwrap();
        assert_eq!(n, HEADER_LEN + 44);
        let (_, dec) = decode_pair_join_req(&buf[..n]).unwrap();
        assert_eq!(dec.join_mac, p.join_mac);
        assert_eq!(dec.join_pub_key, p.join_pub_key);
        assert_eq!(dec.host_mac, p.host_mac);
    }

    #[test]
    fn pair_join_ack_roundtrip() {
        let mut buf = [0u8; 64];
        let p = PairJoinAckPayload {
            host_mac: [1; 6],
            host_pub_key: [2; 32],
            join_mac: [3; 6],
            accepted: 1,
            reason: 0,
        };
        let n = encode_pair_join_ack(&base_header(), &p, &mut buf).unwrap();
        assert_eq!(n, HEADER_LEN + 46);
        let (_, dec) = decode_pair_join_ack(&buf[..n]).unwrap();
        assert_eq!(dec.host_mac, p.host_mac);
        assert_eq!(dec.host_pub_key, p.host_pub_key);
        assert_eq!(dec.join_mac, p.join_mac);
        assert_eq!((dec.accepted, dec.reason), (1, 0));
    }

    #[test]
    fn directory_broadcast_4_members_roundtrip() {
        let mut entries = Vec::new();
        for i in 0..4u8 {
            entries.extend_from_slice(&[i; 6]); // mac
            entries.extend_from_slice(&[i + 100; 32]); // pub_key
        }
        let p = DirectoryBroadcastPayload {
            member_count: 4,
            mode: 1,
            target_channel: 11,
            switch_offset: 0x1234,
            entries: &entries,
        };
        let mut buf = [0u8; 256];
        let n = encode_directory_broadcast(&base_header(), &p, &mut buf).unwrap();
        assert_eq!(n, HEADER_LEN + 5 + 4 * 38);
        // verify switch_offset is BE at offset 11
        assert_eq!(buf[HEADER_LEN + 3], 0x12);
        assert_eq!(buf[HEADER_LEN + 4], 0x34);
        let (_, dec) = decode_directory_broadcast(&buf[..n]).unwrap();
        assert_eq!(dec.member_count, 4);
        assert_eq!(dec.switch_offset, 0x1234);
        for i in 0..4 {
            let e = dec.entry_at(i).unwrap();
            assert_eq!(*e.mac, [i as u8; 6]);
            assert_eq!(*e.pub_key, [i as u8 + 100; 32]);
        }
        assert_eq!(dec.entry_at(4).unwrap_err(), PacketError::Truncated);
    }

    #[test]
    fn directory_broadcast_entry_at_out_of_range() {
        let entries = vec![0u8; 2 * 38];
        let p = DirectoryBroadcastPayload {
            member_count: 2,
            mode: 0,
            target_channel: 0,
            switch_offset: 0,
            entries: &entries,
        };
        assert_eq!(p.entry_at(3).unwrap_err(), PacketError::Truncated);
    }

    #[test]
    fn channel_switch_ack_roundtrip() {
        let mut buf = [0u8; 16];
        let p = ChannelSwitchAckPayload { sender_id: 1, status: 0 };
        let n = encode_channel_switch_ack(&base_header(), &p, &mut buf).unwrap();
        let (_, dec) = decode_channel_switch_ack(&buf[..n]).unwrap();
        assert_eq!((dec.sender_id, dec.status), (1, 0));
    }

    #[test]
    fn decode_voice_truncated_payload() {
        // 8-byte header + 1 byte payload — needs 2
        let buf = [0u8; 9];
        // parse header will succeed (magic 0), but payload len check fails
        // magic = 0 → BadMagic; craft valid magic+header then 1-byte payload
        let mut buf = [0u8; 9];
        buf[0] = 0xC6;
        buf[2] = PacketType::Voice as u8;
        assert_eq!(decode_voice(&buf).unwrap_err(), PacketError::Truncated);
    }
}
