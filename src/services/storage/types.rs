//! Data types for the storage service (change 03/17).
//! Reference: 技术设计 §3.1 / §7.2 / §7.3 / §7.4 / §7.5 / change 03 spec.

use crate::board_profile::BoardProfile;

/// NVS schema version. Bump on incompatible layout change.
/// §7.5 rule: NVS==FW → load; NVS<FW → sys default + group cleanup; NVS>FW → group cleanup.
pub const SCHEMA_VER: u16 = 1;

/// Intercom mode per 技术设计 §3.5. Serialized as u8 (0=Clear, 1=Free).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntercomMode {
    #[default]
    Clear = 0,
    Free = 1,
}

impl IntercomMode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => IntercomMode::Free,
            _ => IntercomMode::Clear,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

impl From<u8> for IntercomMode {
    fn from(v: u8) -> Self {
        Self::from_u8(v)
    }
}
impl From<IntercomMode> for u8 {
    fn from(m: IntercomMode) -> Self {
        m.to_u8()
    }
}

/// System settings (sys namespace). §7.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub schema_ver: u16,
    pub device_name: String,
    pub volume: u8,
    pub muted: bool,
    pub brightness: u8,
    pub screen_off_sec: u32,
}

impl Default for Settings {
    fn default() -> Self {
        // device_name: deterministic placeholder. Real per-device random name
        // generation happens in change 07 (Settings UI) when the user first
        // opens the panel; until then use a stable default.
        let device_name = String::from("INT-0000");
        Self {
            schema_ver: SCHEMA_VER,
            device_name,
            volume: 50,
            muted: false,
            brightness: BoardProfile::DEFAULT_BRIGHTNESS,
            screen_off_sec: BoardProfile::DEFAULT_SCREEN_OFF_SEC,
        }
    }
}

/// One peer in a group. 6 (MAC) + 32 (pub_key) = 38 bytes serialized. §7.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerEntry {
    pub mac: [u8; 6],
    pub pub_key: [u8; 32],
}

impl PeerEntry {
    pub const SERIALIZED_LEN: usize = 38;

    pub fn to_bytes(&self) -> [u8; 38] {
        let mut out = [0u8; 38];
        out[..6].copy_from_slice(&self.mac);
        out[6..].copy_from_slice(&self.pub_key);
        out
    }

    pub fn from_bytes(b: &[u8; 38]) -> Self {
        let mut mac = [0u8; 6];
        let mut pub_key = [0u8; 32];
        mac.copy_from_slice(&b[..6]);
        pub_key.copy_from_slice(&b[6..]);
        Self { mac, pub_key }
    }
}

/// Group info (group namespace). §7.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInfo {
    pub schema_ver: u16,
    pub my_priv_key: [u8; 32],
    pub peers: Vec<PeerEntry>,
    pub mode: IntercomMode,
    pub channel: u8,
    pub last_state: u8,
}

/// Diagnostic info (diag namespace). §7.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagInfo {
    pub abnormal_boot_cnt: u32,
    pub safe_boot_flag: bool,
    pub last_reset_reason: u32,
}

/// Storage error. §3.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageError {
    Io,
    SchemaMismatch,
    Corrupt,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io => write!(f, "storage io error"),
            StorageError::SchemaMismatch => write!(f, "storage schema mismatch"),
            StorageError::Corrupt => write!(f, "storage corrupt"),
        }
    }
}

impl std::error::Error for StorageError {}

/// Serialize a Vec<PeerEntry> to a flat blob (38 * n bytes).
pub fn peers_to_blob(peers: &[PeerEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(peers.len() * PeerEntry::SERIALIZED_LEN);
    for p in peers {
        out.extend_from_slice(&p.to_bytes());
    }
    out
}

/// Deserialize a blob to Vec<PeerEntry>. Blob length must be multiple of 38.
pub fn peers_from_blob(blob: &[u8]) -> Result<Vec<PeerEntry>, StorageError> {
    if blob.len() % PeerEntry::SERIALIZED_LEN != 0 {
        return Err(StorageError::Corrupt);
    }
    let mut out = Vec::with_capacity(blob.len() / PeerEntry::SERIALIZED_LEN);
    for chunk in blob.chunks_exact(PeerEntry::SERIALIZED_LEN) {
        let mut buf = [0u8; 38];
        buf.copy_from_slice(chunk);
        out.push(PeerEntry::from_bytes(&buf));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_entry_roundtrip() {
        let p = PeerEntry {
            mac: [1, 2, 3, 4, 5, 6],
            pub_key: [7u8; 32],
        };
        let bytes = p.to_bytes();
        let p2 = PeerEntry::from_bytes(&bytes);
        assert_eq!(p, p2);
    }

    #[test]
    fn peers_blob_roundtrip() {
        let peers = vec![
            PeerEntry { mac: [1, 2, 3, 4, 5, 6], pub_key: [0xa1u8; 32] },
            PeerEntry { mac: [7, 8, 9, 10, 11, 12], pub_key: [0xa2u8; 32] },
        ];
        let blob = peers_to_blob(&peers);
        assert_eq!(blob.len(), 76);
        let peers2 = peers_from_blob(&blob).unwrap();
        assert_eq!(peers, peers2);
    }

    #[test]
    fn peers_blob_4_max() {
        let peers: Vec<PeerEntry> = (0..4)
            .map(|i| PeerEntry { mac: [i; 6], pub_key: [i; 32] })
            .collect();
        let blob = peers_to_blob(&peers);
        assert_eq!(blob.len(), 4 * 38);
    }

    #[test]
    fn peers_blob_bad_len() {
        let bad = [0u8; 37];
        assert_eq!(peers_from_blob(&bad).unwrap_err(), StorageError::Corrupt);
    }

    #[test]
    fn intercom_mode_roundtrip() {
        assert_eq!(IntercomMode::from_u8(0), IntercomMode::Clear);
        assert_eq!(IntercomMode::from_u8(1), IntercomMode::Free);
        assert_eq!(IntercomMode::from_u8(2), IntercomMode::Clear);
        assert_eq!(IntercomMode::Clear.to_u8(), 0);
        assert_eq!(IntercomMode::Free.to_u8(), 1);
    }

    #[test]
    fn schema_ver_match_loads() {
        assert_eq!(apply_schema_rule(1, 1), SchemaAction::Load);
    }

    #[test]
    fn schema_ver_lower_fallback() {
        assert_eq!(apply_schema_rule(0, 1), SchemaAction::Fallback);
    }

    #[test]
    fn schema_ver_higher_cleanup() {
        assert_eq!(apply_schema_rule(2, 1), SchemaAction::Cleanup);
    }
}

/// Pure-logic schema_ver rule (§7.5). Extracted for unit testing.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SchemaAction {
    Load,
    Fallback,
    Cleanup,
}

pub fn apply_schema_rule(nvs_ver: u16, fw_ver: u16) -> SchemaAction {
    if nvs_ver == fw_ver {
        SchemaAction::Load
    } else if nvs_ver < fw_ver {
        SchemaAction::Fallback
    } else {
        SchemaAction::Cleanup
    }
}
