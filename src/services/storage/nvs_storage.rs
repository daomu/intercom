//! NVS-backed StorageService implementation. change 03/17.
//! design D1: uses esp-idf-svc EspNvs wrapper. Three namespaces: sys / group / diag.
//!
//! NOTE: change 03 ships the trait impl structure with all method bodies
//! following spec §7.5 schema_ver rules + §7.2/§7.3/§7.4 key layouts. The
//! real on-device NVS round-trip is verified when change 07 (Settings UI)
//! and change 09 (pairing) consume this layer. Build-time acceptance
//! (tasks 8.1–8.2) is `cargo build` + `cargo build --release` passing.

#![allow(dead_code)]

use std::fmt;

use esp_idf_svc::nvs::{EspDefaultNvs, EspNvsPartition, NvsDefault};

use crate::services::storage::types::{
    apply_schema_rule, peers_from_blob, peers_to_blob, DiagInfo, GroupInfo, IntercomMode,
    PeerEntry, SchemaAction, Settings, StorageError, SCHEMA_VER,
};
use crate::services::storage::StorageService;

/// NVS-backed storage. Holds a partition handle (Arc-internally, so cheap
/// to clone) and opens a per-namespace `EspDefaultNvs` per operation.
pub struct NvsStorage {
    partition: EspNvsPartition<NvsDefault>,
}

impl fmt::Debug for NvsStorage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NvsStorage").finish_non_exhaustive()
    }
}

impl NvsStorage {
    /// Take the default NVS partition (initializes / reinits as needed) and
    /// construct the storage handle. Returns Err(StorageError::Io) on failure.
    pub fn new() -> Result<Self, StorageError> {
        let partition = EspNvsPartition::<NvsDefault>::take().map_err(|e| {
            log::error!("EspNvsPartition::take failed: {e:?}");
            StorageError::Io
        })?;
        Ok(Self { partition })
    }

    fn open(&self, ns: &str) -> Result<EspDefaultNvs, StorageError> {
        EspNvs::new(self.partition.clone(), ns, true).map_err(|e| {
            log::error!("EspNvs::new({ns}) failed: {e:?}");
            StorageError::Io
        })
    }
}

// NVS setters take &self (not &mut) — the underlying handle is opaque and
// internally synchronized by ESP-IDF, so the helper fns below take &Nvs.

fn read_u16(nvs: &EspDefaultNvs, key: &str) -> Option<u16> {
    nvs.get_u16(key).ok().flatten()
}
fn read_u8(nvs: &EspDefaultNvs, key: &str) -> Option<u8> {
    nvs.get_u8(key).ok().flatten()
}
fn read_u32(nvs: &EspDefaultNvs, key: &str) -> Option<u32> {
    nvs.get_u32(key).ok().flatten()
}
fn read_bool(nvs: &EspDefaultNvs, key: &str) -> Option<bool> {
    nvs.get_u8(key).ok().flatten().map(|v| v != 0)
}
fn read_str(nvs: &EspDefaultNvs, key: &str) -> Option<String> {
    // str_len + get_str with a buffer
    let len = nvs.str_len(key).ok()??;
    let mut buf = vec![0u8; len + 1];
    nvs.get_str(key, &mut buf).ok()?;
    let s = buf.iter().take_while(|c| **c != 0).cloned().collect::<Vec<u8>>();
    String::from_utf8(s).ok()
}
fn read_blob<'a>(nvs: &EspDefaultNvs, key: &str, buf: &'a mut [u8]) -> Option<&'a [u8]> {
    nvs.get_blob(key, buf).ok().flatten()
}

fn write_u16(nvs: &EspDefaultNvs, key: &str, v: u16) -> Result<(), StorageError> {
    nvs.set_u16(key, v).map_err(|_| StorageError::Io)
}
fn write_u8(nvs: &EspDefaultNvs, key: &str, v: u8) -> Result<(), StorageError> {
    nvs.set_u8(key, v).map_err(|_| StorageError::Io)
}
fn write_u32(nvs: &EspDefaultNvs, key: &str, v: u32) -> Result<(), StorageError> {
    nvs.set_u32(key, v).map_err(|_| StorageError::Io)
}
fn write_bool(nvs: &EspDefaultNvs, key: &str, v: bool) -> Result<(), StorageError> {
    nvs.set_u8(key, if v { 1 } else { 0 }).map_err(|_| StorageError::Io)
}
fn write_str(nvs: &EspDefaultNvs, key: &str, v: &str) -> Result<(), StorageError> {
    nvs.set_str(key, v).map_err(|_| StorageError::Io)
}
fn write_blob(nvs: &EspDefaultNvs, key: &str, v: &[u8]) -> Result<(), StorageError> {
    nvs.set_blob(key, v).map_err(|_| StorageError::Io)
}

impl StorageService for NvsStorage {
    fn load_settings(&self) -> Settings {
        let nvs = match self.open("sys") {
            Ok(n) => n,
            Err(_) => return Settings::default(),
        };
        let nvs_ver = read_u16(&nvs, "schema_ver").unwrap_or(0);
        match apply_schema_rule(nvs_ver, SCHEMA_VER) {
            SchemaAction::Load => {
                let device_name = read_str(&nvs, "device_name")
                    .unwrap_or_else(|| Settings::default().device_name);
                let volume = read_u8(&nvs, "volume").unwrap_or_else(|| Settings::default().volume);
                let muted = read_bool(&nvs, "muted").unwrap_or_else(|| Settings::default().muted);
                let brightness =
                    read_u8(&nvs, "brightness").unwrap_or_else(|| Settings::default().brightness);
                let screen_off_sec = read_u32(&nvs, "screen_off_sec")
                    .unwrap_or_else(|| Settings::default().screen_off_sec);
                Settings {
                    schema_ver: SCHEMA_VER,
                    device_name,
                    volume,
                    muted,
                    brightness,
                    screen_off_sec,
                }
            }
            SchemaAction::Fallback | SchemaAction::Cleanup => {
                log::warn!(
                    "sys schema_ver={nvs_ver} vs fw={SCHEMA_VER} → default settings"
                );
                Settings::default()
            }
        }
    }

    fn save_settings(&self, s: &Settings) -> Result<(), StorageError> {
        let nvs = self.open("sys")?;
        write_u16(&nvs, "schema_ver", s.schema_ver)?;
        write_str(&nvs, "device_name", &s.device_name)?;
        write_u8(&nvs, "volume", s.volume)?;
        write_bool(&nvs, "muted", s.muted)?;
        write_u8(&nvs, "brightness", s.brightness)?;
        write_u32(&nvs, "screen_off_sec", s.screen_off_sec)?;
        Ok(())
    }

    fn reset_settings(&self) -> Result<(), StorageError> {
        let nvs = self.open("sys")?;
        for k in ["schema_ver", "device_name", "volume", "muted", "brightness", "screen_off_sec"] {
            let _ = nvs.remove(k);
        }
        Ok(())
    }

    fn load_group(&self) -> Option<GroupInfo> {
        let nvs = self.open("group").ok()?;
        let nvs_ver = read_u16(&nvs, "schema_ver").unwrap_or(0);
        match apply_schema_rule(nvs_ver, SCHEMA_VER) {
            SchemaAction::Load => {}
            SchemaAction::Fallback | SchemaAction::Cleanup => {
                log::warn!("group schema_ver={nvs_ver} vs fw={SCHEMA_VER} → cleanup");
                let _ = self.clear_group();
                return None;
            }
        }
        let mut buf32 = [0u8; 32];
        let priv_bytes = match read_blob(&nvs, "my_priv_key", &mut buf32) {
            Some(b) if b.len() == 32 => {
                let mut k = [0u8; 32];
                k.copy_from_slice(b);
                k
            }
            _ => {
                log::warn!("group my_priv_key missing/wrong len → cleanup");
                let _ = self.clear_group();
                return None;
            }
        };
        let mut peers_buf = [0u8; 4 * PeerEntry::SERIALIZED_LEN];
        let peers = match read_blob(&nvs, "peers", &mut peers_buf) {
            Some(b) => peers_from_blob(b).unwrap_or_default(),
            None => Vec::new(),
        };
        let mode = IntercomMode::from_u8(read_u8(&nvs, "mode").unwrap_or(0));
        let channel = read_u8(&nvs, "channel").unwrap_or(0);
        let last_state = read_u8(&nvs, "last_state").unwrap_or(0);
        Some(GroupInfo {
            schema_ver: SCHEMA_VER,
            my_priv_key: priv_bytes,
            peers,
            mode,
            channel,
            last_state,
        })
    }

    fn save_group(&self, g: &GroupInfo) -> Result<(), StorageError> {
        if g.peers.len() > 4 {
            return Err(StorageError::Corrupt);
        }
        let nvs = self.open("group")?;
        write_u16(&nvs, "schema_ver", g.schema_ver)?;
        write_blob(&nvs, "my_priv_key", &g.my_priv_key)?;
        let peers_blob = peers_to_blob(&g.peers);
        write_blob(&nvs, "peers", &peers_blob)?;
        write_u8(&nvs, "mode", g.mode.to_u8())?;
        write_u8(&nvs, "channel", g.channel)?;
        write_u8(&nvs, "last_state", g.last_state)?;
        Ok(())
    }

    fn clear_group(&self) -> Result<(), StorageError> {
        let nvs = self.open("group")?;
        for k in ["schema_ver", "my_priv_key", "peers", "mode", "channel", "last_state"] {
            let _ = nvs.remove(k);
        }
        Ok(())
    }

    fn load_diag(&self) -> DiagInfo {
        let nvs = match self.open("diag") {
            Ok(n) => n,
            Err(_) => return DiagInfo::default(),
        };
        DiagInfo {
            abnormal_boot_cnt: read_u32(&nvs, "abnormal_boot_cnt").unwrap_or(0),
            safe_boot_flag: read_bool(&nvs, "safe_boot_flag").unwrap_or(false),
            last_reset_reason: read_u32(&nvs, "last_reset_reason").unwrap_or(0),
        }
    }

    fn inc_abnormal_boot(&self) -> Result<(), StorageError> {
        let nvs = self.open("diag")?;
        let cur = read_u32(&nvs, "abnormal_boot_cnt").unwrap_or(0);
        write_u32(&nvs, "abnormal_boot_cnt", cur + 1)?;
        Ok(())
    }

    fn clear_diag(&self) -> Result<(), StorageError> {
        let nvs = self.open("diag")?;
        for k in ["abnormal_boot_cnt", "safe_boot_flag", "last_reset_reason"] {
            let _ = nvs.remove(k);
        }
        Ok(())
    }

    fn set_safe_boot_flag(&self, v: bool) -> Result<(), StorageError> {
        let nvs = self.open("diag")?;
        write_bool(&nvs, "safe_boot_flag", v)
    }

    fn set_last_reset_reason(&self, reason: u32) -> Result<(), StorageError> {
        let nvs = self.open("diag")?;
        write_u32(&nvs, "last_reset_reason", reason)
    }
}

// EspNvsPartition<NvsDefault> is Send (Arc<NvsDefault> where NvsDefault: Send).
// Mark NvsStorage Send + Sync so it can be shared across FreeRTOS tasks via
// the change 01 three-task model.
unsafe impl Send for NvsStorage {}
unsafe impl Sync for NvsStorage {}

// Need EspNvs in scope for the `EspNvs::new` call in `open`.
use esp_idf_svc::nvs::EspNvs;
