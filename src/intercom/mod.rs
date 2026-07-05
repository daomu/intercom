//! Intercom business layer: state machine, packet handlers, pairing,
//! voice PTT, jitter+mixing, heartbeat+restore, voice changer, safety,
//! power management. Filled in by changes 08–16. change 17 integrates.

#![allow(dead_code)]
#![allow(unused_imports)]

pub mod heartbeat;
pub mod jitter;
pub mod pairing;
pub mod packet;
pub mod power_mgmt;
pub mod safety;
pub mod state;
pub mod voice;
pub mod voice_changer;
