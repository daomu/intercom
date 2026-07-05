//! ESP-NOW + Wi-Fi radio driver. design D9.
//!
//! Real wiring: EspWifi (STA mode, no NVS — NVS singleton is owned by
//! NvsStorage) started on the ESP32-C6 Wi-Fi modem, then EspNow::take()
//! initializes ESP-NOW on top. The driver stores both handles to keep
//! them alive. Spec: init() SHALL NOT add_peer / register callback / send
//! packets — that's NetworkService's job.

#![allow(dead_code)]

use esp_idf_svc::espnow::EspNow;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::WifiModemPeripheral;
use esp_idf_svc::wifi::{BlockingWifi, ClientConfiguration, Configuration as WifiConfiguration, EspWifi};

use crate::board_profile::BoardProfile;
use crate::hal::HalError;

pub struct RadioDriver {
    _wifi: BlockingWifi<EspWifi<'static>>,
    espnow: EspNow<'static>,
    channel: u8,
}

impl std::fmt::Debug for RadioDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioDriver")
            .field("channel", &self.channel)
            .finish_non_exhaustive()
    }
}

impl RadioDriver {
    /// Construct from owned Wi-Fi modem peripheral. Initializes Wi-Fi in
    /// STA mode (no AP connection — ESP-NOW doesn't need it), then ESP-NOW.
    /// NVS is passed as `None` — the NVS singleton is owned by NvsStorage.
    pub fn init<MODEM>(modem: MODEM) -> Result<Self, HalError>
    where
        MODEM: WifiModemPeripheral + 'static,
    {
        let sysloop = EspSystemEventLoop::take()
            .map_err(|e| HalError::RadioInitFailed(format!("event loop: {e}")))?;

        // Initialize Wi-Fi in STA mode without NVS (NVS singleton owned by NvsStorage).
        let wifi = EspWifi::new(modem, sysloop.clone(), None)
            .map_err(|e| HalError::RadioInitFailed(format!("EspWifi: {e}")))?;
        let mut wifi = BlockingWifi::wrap(wifi, sysloop)
            .map_err(|e| HalError::RadioInitFailed(format!("BlockingWifi: {e}")))?;

        // Set STA mode (ESP-NOW requires STA or AP+STA).
        wifi.set_configuration(&WifiConfiguration::Client(ClientConfiguration::default()))
            .map_err(|e| HalError::RadioInitFailed(format!("set_configuration: {e}")))?;
        wifi.start()
            .map_err(|e| HalError::RadioInitFailed(format!("wifi start: {e}")))?;

        // Initialize ESP-NOW on top of the started Wi-Fi driver.
        let espnow = EspNow::take()
            .map_err(|e| HalError::RadioInitFailed(format!("EspNow: {e}")))?;

        Ok(Self {
            _wifi: wifi,
            espnow,
            channel: BoardProfile::DISCOVERY_CHANNEL,
        })
    }

    /// Borrow the ESP-NOW handle for NetworkService to use (add_peer, send, etc.).
    pub fn espnow(&self) -> &EspNow<'static> {
        &self.espnow
    }

    pub fn channel(&self) -> u8 {
        self.channel
    }
}
