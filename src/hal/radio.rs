//! ESP-NOW + Wi-Fi radio driver. design D9.
//!
//! NOTE: change 02 stubs EspWifi + EspNow construction. Real init (EspWifi
//! STA mode, EspNow::new on channel=DISCOVERY_CHANNEL) is added in change
//! 06 (NetworkService) when add_peer / receive callback / packet TX are
//! wired up. Spec: SHALL NOT add_peer / register callback / send packets.

#![allow(dead_code)]

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct RadioDriver {
    _channel: u8,
}

impl RadioDriver {
    pub fn init() -> Result<Self, HalError> {
        Ok(Self {
            _channel: BoardProfile::DISCOVERY_CHANNEL,
        })
    }
}
