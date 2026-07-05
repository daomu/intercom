//! Storage service: trait + NVS impl. change 03/17.
//! Reference: 技术设计 §3.1 / §7 / change 03 design D1-D3, D7, D10.

#![allow(dead_code)]

pub mod nvs_storage;
pub mod types;

pub use nvs_storage::NvsStorage;
pub use types::{
    apply_schema_rule, peers_from_blob, peers_to_blob, DiagInfo, GroupInfo, IntercomMode,
    PeerEntry, SchemaAction, Settings, StorageError, SCHEMA_VER,
};

use std::fmt::Debug;

/// Storage service trait (§3.1). Send + Sync so it can be shared across tasks.
pub trait StorageService: Send + Sync + Debug {
    fn load_settings(&self) -> Settings;
    fn save_settings(&self, s: &Settings) -> Result<(), StorageError>;
    fn reset_settings(&self) -> Result<(), StorageError>;
    fn load_group(&self) -> Option<GroupInfo>;
    fn save_group(&self, g: &GroupInfo) -> Result<(), StorageError>;
    fn clear_group(&self) -> Result<(), StorageError>;
    fn load_diag(&self) -> DiagInfo;
    fn inc_abnormal_boot(&self) -> Result<(), StorageError>;
    fn clear_diag(&self) -> Result<(), StorageError>;
}
