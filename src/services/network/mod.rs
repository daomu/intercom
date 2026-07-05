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
    /// Received signal strength indicator (dBm). ESP-NOW fetches this from
    /// the Wi-Fi RX descriptor on receive. Default -100 dBm when unknown.
    pub rssi: i8,
}

impl RecvEvent {
    pub fn new(src_mac: [u8; 6], payload: Vec<u8>, rssi: i8) -> Self {
        Self { src_mac, payload, rssi }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    EspNow,
    InvalidParam,
    PeerLimit,
    NotInitialized,
    /// Radio guard prevented the operation (D13: radio_priority == 0).
    Channel,
    /// Unicast peer not registered.
    TxFail,
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::EspNow => write!(f, "espnow error"),
            NetError::InvalidParam => write!(f, "invalid param"),
            NetError::PeerLimit => write!(f, "peer limit exceeded"),
            NetError::NotInitialized => write!(f, "not initialized"),
            NetError::Channel => write!(f, "channel/radio guard rejected"),
            NetError::TxFail => write!(f, "tx failed (peer unregistered or radio busy)"),
        }
    }
}
impl std::error::Error for NetError {}

/// Network service trait. §3.3. design D13/D18/D23.
pub trait NetworkService: Send + Sync + fmt::Debug {
    fn init(&self, channel: u8) -> Result<(), NetError>;
    fn set_channel(&self, channel: u8) -> Result<(), NetError>;
    fn add_peer(&self, mac: &[u8; 6], lmk: Option<&[u8; 16]>) -> Result<(), NetError>;
    fn remove_peer(&self, mac: &[u8; 6]) -> Result<(), NetError>;
    fn clear_peers(&self) -> Result<(), NetError>;

    /// Encrypted unicast to a registered peer. Returns `TxFail` if dst is
    /// not in the peer list.
    fn send_unicast(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError>;
    /// Broadcast on the discovery channel (channel 1). Returns `Channel`
    /// if current channel != 1 (D13: broadcast is discovery-only).
    fn send_broadcast(&self, payload: &[u8]) -> Result<(), NetError>;

    /// Register a single receive callback (replaces any prior).
    fn on_recv(&self, cb: Box<dyn Fn(RecvEvent) + Send + Sync>);

    /// Evaluate candidate channels and return the one with highest RSSI.
    /// Returns `Channel` if radio_priority == 0 (D13). Empty candidates
    /// returns `Ok(current_channel)`.
    fn evaluate_channels(&self, candidates: &[u8]) -> Result<u8, NetError>;
    /// Latest known RSSI (dBm). -100 when no signal info available.
    fn get_rssi(&self) -> i8;
    /// Radio guard priority (0 = block channel changes, >0 = allow).
    /// Stored as AtomicU8 internally so any task can set it.
    fn set_radio_priority(&self, p: u8);

    fn current_channel(&self) -> u8;
    fn peer_count(&self) -> u8;

    // ---- Legacy aliases (kept for callers that predate the rename) ----
    /// Alias for `send_unicast`.
    fn send(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError> {
        self.send_unicast(dst, payload)
    }
    /// Alias for `send_broadcast`.
    fn broadcast(&self, payload: &[u8]) -> Result<(), NetError> {
        self.send_broadcast(payload)
    }
}

/// Stub impl. Real EspNow wiring (from change 02 RadioDriver) deferred to
/// on-hardware verification.
pub struct EspNowNetworkServiceStub {
    channel: u8,
    rssi: std::sync::atomic::AtomicI8,
    radio_priority: std::sync::atomic::AtomicU8,
}

impl fmt::Debug for EspNowNetworkServiceStub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EspNowNetworkServiceStub")
            .field("channel", &self.channel)
            .field("rssi", &self.rssi.load(std::sync::atomic::Ordering::Relaxed))
            .field("radio_priority", &self.radio_priority.load(std::sync::atomic::Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl EspNowNetworkServiceStub {
    pub fn new() -> Self {
        Self {
            channel: BoardProfile::DISCOVERY_CHANNEL,
            rssi: std::sync::atomic::AtomicI8::new(-100),
            radio_priority: std::sync::atomic::AtomicU8::new(1),
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
    fn set_channel(&self, channel: u8) -> Result<(), NetError> {
        if self.radio_priority.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(NetError::Channel);
        }
        let _ = channel;
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
    fn send_unicast(&self, _dst: &[u8; 6], _payload: &[u8]) -> Result<(), NetError> {
        Ok(())
    }
    fn send_broadcast(&self, _payload: &[u8]) -> Result<(), NetError> {
        if self.channel != 1 {
            return Err(NetError::Channel);
        }
        Ok(())
    }
    fn on_recv(&self, _cb: Box<dyn Fn(RecvEvent) + Send + Sync>) {}
    fn evaluate_channels(&self, candidates: &[u8]) -> Result<u8, NetError> {
        if self.radio_priority.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            return Err(NetError::Channel);
        }
        if candidates.is_empty() {
            return Ok(self.channel);
        }
        // Stub: return first candidate (real impl would scan RSSI).
        Ok(candidates[0])
    }
    fn get_rssi(&self) -> i8 {
        self.rssi.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_radio_priority(&self, p: u8) {
        self.radio_priority.store(p, std::sync::atomic::Ordering::SeqCst);
    }
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

use std::sync::atomic::{AtomicI8, AtomicU8, Ordering::SeqCst};
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
    /// Most recent RSSI seen on a recv (AtomicI8 for lock-free read).
    last_rssi: AtomicI8,
    /// Radio guard priority (D13: 0 = block channel changes).
    radio_priority: AtomicU8,
    /// Peer MAC list for unicast validation (D18).
    peers: Mutex<Vec<[u8; 6]>>,
    recv_cb: RecvCb,
}

impl fmt::Debug for EspNowNetworkService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EspNowNetworkService")
            .field("channel", &self.channel)
            .field("peer_count", &self.peer_count)
            .field("last_rssi", &self.last_rssi.load(SeqCst))
            .field("radio_priority", &self.radio_priority.load(SeqCst))
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
            last_rssi: AtomicI8::new(-100),
            radio_priority: AtomicU8::new(1),
            peers: Mutex::new(Vec::new()),
            recv_cb: Arc::new(Mutex::new(None)),
        };
        // Register a stub recv callback that forwards to user-supplied cb.
        // The FFI callback is FnMut + 'static; we transmute lifetime via Arc.
        let cb_clone = svc.recv_cb.clone();
        let rssi_ptr = &svc.last_rssi as *const AtomicI8 as usize;
        let _ = svc.espnow.register_recv_cb(move |info, data| {
            // Update last RSSI from the recv info's rssi field if available.
            // The ESP-NOW recv callback gives src/dst but no rssi in the
            // esp-idf-svc wrapper; real rssi fetch uses esp_wifi_sta_get_rssi.
            // We leave the cached value untouched here; get_rssi() pulls
            // from esp_wifi_sta_get_rssi at call time.
            let _ = (info, rssi_ptr);
            // Task 6.2: catch_unwind around user callback — a panic in the
            // user callback must not propagate through the ESP-NOW FFI thread.
            let cb_guard = cb_clone.lock().ok();
            if let Some(guard) = cb_guard {
                if let Some(cb) = guard.as_ref() {
                    let evt = RecvEvent::new(*info.src_addr, data.to_vec(), -100);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        cb(evt);
                    }));
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

    fn peer_known(&self, mac: &[u8; 6]) -> bool {
        self.peers.lock().unwrap().iter().any(|m| m == mac)
    }
}

impl NetworkService for EspNowNetworkService {
    fn init(&self, channel: u8) -> Result<(), NetError> {
        *self.channel.lock().unwrap() = channel;
        Ok(())
    }
    fn set_channel(&self, channel: u8) -> Result<(), NetError> {
        if self.radio_priority.load(SeqCst) == 0 {
            return Err(NetError::Channel);
        }
        // NOTE: runtime channel change requires Wi-Fi re-association; we only
        // cache the value for use in subsequent add_peer calls.
        *self.channel.lock().unwrap() = channel;
        Ok(())
    }
    fn add_peer(&self, mac: &[u8; 6], lmk: Option<&[u8; 16]>) -> Result<(), NetError> {
        let peer = self.make_peer(mac, lmk);
        self.espnow
            .add_peer(peer)
            .map_err(|_| NetError::PeerLimit)?;
        self.peers.lock().unwrap().push(*mac);
        let mut c = self.peer_count.lock().unwrap();
        *c = c.saturating_add(1);
        Ok(())
    }
    fn remove_peer(&self, mac: &[u8; 6]) -> Result<(), NetError> {
        self.espnow
            .del_peer(*mac)
            .map_err(|_| NetError::EspNow)?;
        let mut peers = self.peers.lock().unwrap();
        peers.retain(|m| m != mac);
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
        self.peers.lock().unwrap().clear();
        *self.peer_count.lock().unwrap() = 0;
        Ok(())
    }
    fn send_unicast(&self, dst: &[u8; 6], payload: &[u8]) -> Result<(), NetError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(NetError::InvalidParam);
        }
        if !self.peer_known(dst) {
            return Err(NetError::TxFail);
        }
        self.espnow
            .send(*dst, payload)
            .map_err(|_| NetError::TxFail)
    }
    fn send_broadcast(&self, payload: &[u8]) -> Result<(), NetError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(NetError::InvalidParam);
        }
        if *self.channel.lock().unwrap() != 1 {
            return Err(NetError::Channel);
        }
        self.espnow
            .send(BROADCAST, payload)
            .map_err(|_| NetError::EspNow)
    }
    fn on_recv(&self, cb: Box<dyn Fn(RecvEvent) + Send + Sync>) {
        *self.recv_cb.lock().unwrap() = Some(cb);
    }
    fn evaluate_channels(&self, candidates: &[u8]) -> Result<u8, NetError> {
        if self.radio_priority.load(SeqCst) == 0 {
            return Err(NetError::Channel);
        }
        if candidates.is_empty() {
            return Ok(self.current_channel());
        }
        // For each candidate: cache channel, sample RSSI after 20ms dwell,
        // pick the best. set_channel is a no-op on the radio (Wi-Fi re-association
        // required), but we still perform the 20ms dwell per spec so that
        // future radio implementations with real channel switching work.
        let original = *self.channel.lock().unwrap();
        let mut best_ch = candidates[0];
        let mut best_rssi = -128i8;
        for &ch in candidates {
            *self.channel.lock().unwrap() = ch;
            // 20ms dwell to let RSSI settle on the new channel.
            esp_idf_svc::hal::delay::FreeRtos::delay_ms(20);
            let r = self.get_rssi();
            if r > best_rssi {
                best_rssi = r;
                best_ch = ch;
            }
        }
        // Restore original channel.
        *self.channel.lock().unwrap() = original;
        Ok(best_ch)
    }
    fn get_rssi(&self) -> i8 {
        // esp_wifi_sta_get_rssi(out: *mut c_int) -> esp_err_t
        // SAFETY: esp_wifi is started by RadioDriver::init before this
        // service is constructed. We pass a valid out-pointer.
        let mut raw: i32 = 0;
        let err = unsafe { esp_idf_svc::sys::esp_wifi_sta_get_rssi(&mut raw as *mut i32) };
        let r = if err == 0 { raw as i8 } else { -100i8 };
        self.last_rssi.store(r, SeqCst);
        r
    }
    fn set_radio_priority(&self, p: u8) {
        self.radio_priority.store(p, SeqCst);
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
