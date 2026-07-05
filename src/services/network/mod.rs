//! Network service: ESP-NOW packet format + add_peer + send/recv. change 06/17.
//! Spec: §3.3. design D1-D5 (see change 06 design.md).

#![allow(dead_code)]
#![allow(unused_imports)]

use std::fmt;
use std::sync::Mutex;

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

// ---- Real HAL impl (EspNow via RadioDriver) ------------------------------

use std::sync::Arc;

use esp_idf_svc::espnow::{EspNow, PeerInfo, BROADCAST};
use esp_idf_svc::sys::wifi_interface_t_WIFI_IF_STA;

/// Callback storage for `register_recv_cb`. The EspNow FFI callback is
/// `FnMut` + `'static`, so we wrap user callback in `Arc<Mutex<...>>` to
/// share across the FFI boundary.
type RecvCb = Arc<Mutex<Option<Box<dyn Fn(RecvEvent) + Send + Sync>>>>;

/// Real network service backed by `EspNow` (from `RadioDriver`).
///
/// Channel changes are NOT applied at runtime — ESP-NOW inherits the Wi-Fi
/// channel set by `RadioDriver::init()` and changing it after start requires
/// a Wi-Fi re-association. `set_channel` here is a no-op for the radio; it
/// only updates the cached value used when adding peers.
pub struct EspNowNetworkService {
    espnow: EspNow<'static>,
    channel: Mutex<u8>,
    peer_count: Mutex<u8>,
    recv_cb: RecvCb,
}

impl fmt::Debug for EspNowNetworkService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EspNowNetworkService")
            .field("channel", &self.channel)
            .field("peer_count", &self.peer_count)
            .finish_non_exhaustive()
    }
}

impl EspNowNetworkService {
    /// Construct from an owned `EspNow<'static>` (taken from `RadioDriver`
    /// via `RadioDriver::take_espnow()`). The handle is `EspNow<'static>`
    /// backed by a global singleton.
    pub fn new(espnow: EspNow<'static>) -> Self {
        let channel = BoardProfile::DISCOVERY_CHANNEL;
        let svc = Self {
            espnow,
            channel: Mutex::new(channel),
            peer_count: Mutex::new(0),
            recv_cb: Arc::new(Mutex::new(None)),
        };
        // Register a stub recv callback that forwards to user-supplied cb.
        // The FFI callback is FnMut + 'static; we transmute lifetime via Arc.
        let cb_clone = svc.recv_cb.clone();
        let _ = svc.espnow.register_recv_cb(move |info, data| {
            let cb_guard = cb_clone.lock().ok();
            if let Some(guard) = cb_guard {
                if let Some(cb) = guard.as_ref() {
                    let evt = RecvEvent {
                        src_mac: *info.src_addr,
                        payload: data.to_vec(),
                    };
                    cb(evt);
                }
            }
        });
        svc
    }

    fn make_peer(&self, mac: &[u8; 6], lmk: Option<&[u8; 16]>) -> PeerInfo {
        let mut p = PeerInfo::default();
        p.peer_addr = *mac;
        p.channel = *self.channel.lock().unwrap();
        p.ifidx = wifi_interface_t_WIFI_IF_STA;
        p.encrypt = lmk.is_some();
        if let Some(k) = lmk {
            p.lmk = *k;
        }
        p
    }
}

impl NetworkService for EspNowNetworkService {
    fn init(&self, channel: u8) -> Result<(), NetError> {
        *self.channel.lock().unwrap() = channel;
        Ok(())
    }
    fn set_channel(&self, channel: u8) -> Result<(), NetError> {
        // NOTE: runtime channel change requires Wi-Fi re-association; we only
        // cache the value for use in subsequent add_peer calls.
        *self.channel.lock().unwrap() = channel;
        Ok(())
    }
    fn add_peer(&self, mac: &[u8; 6], lmk: Option<&[u8; 16]>) -> Result<(), NetError> {
        let peer = self.make_peer(mac, lmk);
        self.espnow
            .add_peer(peer)
            .map_err(|_| NetError::EspNow)?;
        let mut c = self.peer_count.lock().unwrap();
        *c = c.saturating_add(1);
        Ok(())
    }
    fn remove_peer(&self, mac: &[u8; 6]) -> Result<(), NetError> {
        self.espnow
            .del_peer(*mac)
            .map_err(|_| NetError::EspNow)?;
        let mut c = self.peer_count.lock().unwrap();
        *c = c.saturating_sub(1);
        Ok(())
    }
    fn clear_peers(&self) -> Result<(), NetError> {
        // Iterate and remove all peers.
        while let Ok((total, _)) = self.espnow.get_peers_number() {
            if total == 0 {
                break;
            }
            if let Ok(peer) = self.espnow.fetch_peer(true) {
                let _ = self.espnow.del_peer(peer.peer_addr);
            } else {
                break;
            }
        }
        *self.peer_count.lock().unwrap() = 0;
        Ok(())
    }
    fn send(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(NetError::InvalidParam);
        }
        self.espnow
            .send(*dst, payload)
            .map_err(|_| NetError::EspNow)
    }
    fn broadcast(&self, payload: &[u8]) -> Result<(), NetError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(NetError::InvalidParam);
        }
        self.espnow
            .send(BROADCAST, payload)
            .map_err(|_| NetError::EspNow)
    }
    fn on_recv(&self, cb: Box<dyn Fn(RecvEvent) + Send + Sync>) {
        *self.recv_cb.lock().unwrap() = Some(cb);
    }
    fn current_channel(&self) -> u8 {
        *self.channel.lock().unwrap()
    }
    fn peer_count(&self) -> u8 {
        *self.peer_count.lock().unwrap()
    }
}

unsafe impl Send for EspNowNetworkService {}
unsafe impl Sync for EspNowNetworkService {}
