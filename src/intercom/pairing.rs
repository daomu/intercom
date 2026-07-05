//! Three-phase pairing state machine (change 09). Spec: §3.5, §8, §19.1/§19.2.
//!
//! Pure-logic state machine covering Host path (Discovering →
//! CollectingPeers → Frozen → SwitchingChannel → Grouped) and Join path
//! (Searching → Requesting → WaitingConfirm → SwitchingChannel → Grouped).
//! Side effects (send packets, clear_peers, set_channel, add_peer,
//! save_group, schedule timers) are emitted as `PairingAction`s so the
//! caller (IntercomService on Task B) can execute them.

#![allow(dead_code)]

use std::fmt;

use crate::board_profile::BoardProfile;
use crate::intercom::state::{
    HostPhase, IntercomMode, IntercomState, JoinPhase, VoiceState,
};

/// Schema version mismatch reason code (D10 / §19.2).
pub const PAIR_JOIN_REASON_INCOMPATIBLE: u8 = BoardProfile::PAIR_JOIN_REASON_INCOMPATIBLE;

// ---- Failure codes (D10) -------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingFailure {
    SearchTimeout,
    HostEnded,
    GroupFull,
    SchemaIncompatible,
    SignalWeak,
    StateChanged,
    HostTimeout,
}

impl fmt::Display for PairingFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingFailure::SearchTimeout => write!(f, "search timeout"),
            PairingFailure::HostEnded => write!(f, "host ended"),
            PairingFailure::GroupFull => write!(f, "group full"),
            PairingFailure::SchemaIncompatible => write!(f, "schema incompatible"),
            PairingFailure::SignalWeak => write!(f, "signal weak"),
            PairingFailure::StateChanged => write!(f, "state changed"),
            PairingFailure::HostTimeout => write!(f, "host timeout"),
        }
    }
}

/// PAIR_JOIN_ACK reason codes (D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinAckReason {
    Accepted = 0,
    Full = 1,
    SchemaIncompatible = 2,
    StateChanged = 3,
}

// ---- Events --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum PairingEvent {
    /// User: start hosting with a mode.
    StartHost { mode: IntercomMode, max_members: u8 },
    /// User: start searching for hosts.
    StartJoin,
    /// User: cancel (only valid in Discovering / CollectingPeers / Searching / Requesting).
    Cancel,
    /// Host: confirm collected peers → freeze + schedule DIRECTORY_BROADCAST ×5.
    HostConfirm { switch_offset_ms: u16, target_channel: u8 },
    /// Join: select a host from the discovered list.
    SelectHost { host_mac: [u8; 6] },

    /// Network: received a host beacon.
    BeaconReceived {
        host_mac: [u8; 6],
        host_pub_key: [u8; 32],
        mode: IntercomMode,
        cur_members: u8,
        max_members: u8,
        joinable: bool,
        rssi_4bar: u8,
    },
    /// Network: Host received a join request.
    /// `ver` is the schema version from the PAIR_JOIN_REQ header (task 2.3).
    JoinReqReceived {
        join_mac: [u8; 6],
        join_pub_key: [u8; 32],
        ver: u8,
    },
    /// Network: Join received a join ack.
    JoinAckReceived {
        host_mac: [u8; 6],
        host_pub_key: [u8; 32],
        accepted: bool,
        reason: JoinAckReason,
    },
    /// Network: Join received DIRECTORY_BROADCAST.
    DirectoryBroadcastReceived {
        member_count: u8,
        mode: IntercomMode,
        target_channel: u8,
        switch_offset_ms: u16,
        members: Vec<([u8; 6], [u8; 32])>,
    },
    /// Network: Host received CHANNEL_SWITCH_ACK.
    ChannelSwitchAckReceived { sender_id: u8, status: u8 },

    /// Timer: 200ms beacon tick (Host).
    BeaconTick,
    /// Timer: 90s host confirm timeout.
    HostConfirmTimeout,
    /// Timer: 3s search timeout (Join).
    SearchTimeout,
    /// Timer: 2s ACK wait (Host).
    AckTimeout,
    /// Timer: scheduled DIRECTORY_BROADCAST (one of 5).
    DirectoryBroadcastTick { index: u8 },
    /// Timer: switch deadline reached.
    SwitchDeadline,
}

// ---- Actions -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum PairingAction {
    SendBeacon {
        host_mac: [u8; 6],
        host_pub_key: [u8; 32],
        mode: IntercomMode,
        cur_members: u8,
        max_members: u8,
        joinable: bool,
    },
    SendJoinReq {
        join_mac: [u8; 6],
        join_pub_key: [u8; 32],
        host_mac: [u8; 6],
    },
    SendJoinAck {
        host_mac: [u8; 6],
        host_pub_key: [u8; 32],
        join_mac: [u8; 6],
        accepted: bool,
        reason: JoinAckReason,
    },
    SendDirectoryBroadcast {
        member_count: u8,
        mode: IntercomMode,
        target_channel: u8,
        switch_offset_ms: u16,
        members: Vec<([u8; 6], [u8; 32])>,
    },
    SendChannelSwitchAck { status: u8 },
    ClearPeers,
    SetChannel { channel: u8 },
    /// Add a peer with the peer's public key; caller derives the LMK via
    /// `CryptoService::derive_lmk(my_priv, peer_pub)` and calls
    /// `NetworkService::add_peer(mac, Some(&lmk))`.
    AddPeer { mac: [u8; 6], peer_pub_key: [u8; 32] },
    SaveGroup {
        info: PairingGroupInfo,
    },
    ScheduleDirectoryBroadcasts,
    ScheduleSwitch { offset_ms: u16 },
    Fail(PairingFailure),
    EnterGrouped,
    DiscoveredHostsUpdate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairingOutcome {
    pub new_state: IntercomState,
    pub actions: Vec<PairingAction>,
}

// ---- Discovered host entry (D9) ------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredHost {
    pub host_mac: [u8; 6],
    pub host_pub_key: [u8; 32],
    pub mode: IntercomMode,
    pub cur_members: u8,
    pub max_members: u8,
    pub joinable: bool,
    pub rssi_4bar: u8,
    pub last_seen_ms: u64,
}

// ---- HostInfo (D9: UI data contract) -------------------------------------

/// UI-facing host descriptor (task 1.4). `name` defaults to
/// `"Host-" + mac_suffix4` when the host didn't set a device name.
#[derive(Debug, Clone, PartialEq)]
pub struct HostInfo {
    pub name: String,
    pub mac_suffix4: String,
    pub rssi_4bar: u8,
    pub cur_members: u8,
    pub max_members: u8,
    pub mode: IntercomMode,
    pub joinable: bool,
    pub last_seen_ms: u64,
}

impl HostInfo {
    /// Build a `HostInfo` from a `DiscoveredHost`, generating the default
    /// name and mac_suffix4 string (task 8.3 / 8.4).
    pub fn from_discovered(h: &DiscoveredHost) -> Self {
        let suffix = format_mac_suffix4(&h.host_mac);
        Self {
            name: format!("Host-{}", suffix),
            mac_suffix4: suffix,
            rssi_4bar: h.rssi_4bar,
            cur_members: h.cur_members,
            max_members: h.max_members,
            mode: h.mode,
            joinable: h.joinable,
            last_seen_ms: h.last_seen_ms,
        }
    }
}

// ---- GroupInfo (task 1.5: save_group payload) ----------------------------

/// Group descriptor emitted in `SaveGroup` so the caller can persist it
/// via `StorageService::save_group`. Carries the data the caller needs to
/// reconstruct `storage::GroupInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct PairingGroupInfo {
    pub my_priv_key: [u8; 32],
    pub peers: Vec<([u8; 6], [u8; 32])>,
    pub channel: u8,
    pub mode: IntercomMode,
}

// ---- Helpers (task 8.2 / 8.3 / 8.4) --------------------------------------

/// Format the last 4 bytes of a MAC as a lowercase hex string (task 8.3).
/// e.g. `[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]` → `"eeff"`.
pub fn format_mac_suffix4(mac: &[u8; 6]) -> String {
    format!("{:02x}{:02x}", mac[4], mac[5])
}

/// RSSI → 4-bar mapping with ±3 dB hysteresis (task 8.2 / §13.2).
/// Delegates to `heartbeat::rssi_to_bars` to reuse the tested hysteresis
/// implementation.
pub fn rssi_to_4bar(smoothed_rssi: i32, current_bars: u8) -> u8 {
    crate::intercom::heartbeat::rssi_to_bars(smoothed_rssi, current_bars)
}

/// Validate a peer public key (task 2.3: non all-0, non all-0xFF, len 32).
pub fn is_valid_pub_key(pub_key: &[u8; 32]) -> bool {
    let all_zero = pub_key.iter().all(|&b| b == 0);
    let all_ff = pub_key.iter().all(|&b| b == 0xFF);
    !(all_zero || all_ff)
}

// ---- Machine -------------------------------------------------------------

pub struct PairingMachine {
    state: IntercomState,
    my_mac: [u8; 6],
    my_pub_key: [u8; 32],
    /// Private key — caller fills at construction (or via `set_my_priv_key`).
    /// Needed for `SaveGroup` and LMK derivation by caller.
    my_priv_key: [u8; 32],
    mode: IntercomMode,
    max_members: u8,
    /// Collected peers (Host path) or selected host (Join path).
    members: Vec<([u8; 6], [u8; 32])>,
    /// Join discovered hosts list.
    discovered_hosts: Vec<DiscoveredHost>,
    /// Selected host for Join path.
    selected_host: Option<[u8; 6]>,
    /// Frozen target channel + switch offset (Host path).
    target_channel: u8,
    switch_offset_ms: u16,
    /// Count of CHANNEL_SWITCH_ACKs received during SwitchingChannel (Host).
    acks_received: u8,
    /// Count of DIRECTORY_BROADCAST sent (Host).
    directory_broadcasts_sent: u8,
    /// Radio busy flag (D4.5 / D6.6): true while DIRECTORY_BROADCAST +
    /// channel switch is in progress. Blocks other radio operations.
    radio_busy: bool,
}

impl fmt::Debug for PairingMachine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingMachine")
            .field("state", &self.state)
            .field("mode", &self.mode)
            .field("members", &self.members.len())
            .finish_non_exhaustive()
    }
}

impl PairingMachine {
    pub fn new(my_mac: [u8; 6], my_pub_key: [u8; 32]) -> Self {
        Self::with_priv_key(my_mac, my_pub_key, [0u8; 32])
    }

    /// Construct with the private key filled in (needed for SaveGroup).
    pub fn with_priv_key(my_mac: [u8; 6], my_pub_key: [u8; 32], my_priv_key: [u8; 32]) -> Self {
        Self {
            state: IntercomState::Idle,
            my_mac,
            my_pub_key,
            my_priv_key,
            mode: IntercomMode::Clear,
            max_members: 4,
            members: Vec::new(),
            discovered_hosts: Vec::new(),
            selected_host: None,
            target_channel: 0,
            switch_offset_ms: 0,
            acks_received: 0,
            directory_broadcasts_sent: 0,
            radio_busy: false,
        }
    }

    /// Set/update the private key (caller obtains it from CryptoService).
    pub fn set_my_priv_key(&mut self, k: [u8; 32]) {
        self.my_priv_key = k;
    }

    pub fn state(&self) -> &IntercomState {
        &self.state
    }

    pub fn discovered_hosts(&self) -> &[DiscoveredHost] {
        &self.discovered_hosts
    }

    /// UI-facing host list (task 8.1: `get_host_list() -> Vec<HostInfo>`).
    pub fn host_list(&self) -> Vec<HostInfo> {
        self.discovered_hosts
            .iter()
            .map(HostInfo::from_discovered)
            .collect()
    }

    pub fn members(&self) -> &[([u8; 6], [u8; 32])] {
        &self.members
    }

    pub fn radio_busy(&self) -> bool {
        self.radio_busy
    }

    /// Add or update a discovered host (D9: dedup by host_mac, 5s expiry
    /// enforced by caller via last_seen_ms + prune_stale()).
    fn upsert_host(&mut self, host: DiscoveredHost) {
        if let Some(existing) = self
            .discovered_hosts
            .iter_mut()
            .find(|h| h.host_mac == host.host_mac)
        {
            *existing = host;
        } else {
            self.discovered_hosts.push(host);
        }
    }

    pub fn prune_stale(&mut self, now_ms: u64, max_age_ms: u64) {
        self.discovered_hosts.retain(|h| now_ms - h.last_seen_ms < max_age_ms);
    }

    pub fn handle(&mut self, event: PairingEvent) -> PairingOutcome {
        let mut actions: Vec<PairingAction> = Vec::new();
        let prev_state = self.state.clone();

        match (prev_state.clone(), event) {
            // ---- Idle ----
            (IntercomState::Idle, PairingEvent::StartHost { mode, max_members }) => {
                self.mode = mode;
                self.max_members = max_members;
                self.members.clear();
                self.state = IntercomState::Hosting(HostPhase::Discovering);
            }
            (IntercomState::Idle, PairingEvent::StartJoin) => {
                self.discovered_hosts.clear();
                self.selected_host = None;
                self.state = IntercomState::Joining(JoinPhase::Searching);
            }

            // ---- Host: Discovering / CollectingPeers ----
            (
                IntercomState::Hosting(HostPhase::Discovering),
                PairingEvent::BeaconTick,
            ) => {
                // Send beacon advertising the host.
                actions.push(PairingAction::SendBeacon {
                    host_mac: self.my_mac,
                    host_pub_key: self.my_pub_key,
                    mode: self.mode,
                    cur_members: self.members.len() as u8,
                    max_members: self.max_members,
                    joinable: self.members.len() < self.max_members as usize,
                });
            }
            (
                IntercomState::Hosting(HostPhase::Discovering),
                PairingEvent::JoinReqReceived { join_mac, join_pub_key, ver },
            ) => {
                // Schema version check (task 2.3): ver must match SCHEMA_VER.
                if ver as u16 != BoardProfile::SCHEMA_VER {
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::SchemaIncompatible,
                    });
                } else if !is_valid_pub_key(&join_pub_key) {
                    // pub_key validity check (task 2.3: non all-0/0xFF).
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::SchemaIncompatible,
                    });
                } else if self.members.len() >= (self.max_members as usize).saturating_sub(1) {
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::Full,
                    });
                } else {
                    self.members.push((join_mac, join_pub_key));
                    // Move to CollectingPeers after first join.
                    self.state = IntercomState::Hosting(HostPhase::CollectingPeers);
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: true,
                        reason: JoinAckReason::Accepted,
                    });
                }
            }
            (
                IntercomState::Hosting(HostPhase::CollectingPeers),
                PairingEvent::JoinReqReceived { join_mac, join_pub_key, ver },
            ) => {
                if ver as u16 != BoardProfile::SCHEMA_VER {
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::SchemaIncompatible,
                    });
                } else if !is_valid_pub_key(&join_pub_key) {
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::SchemaIncompatible,
                    });
                } else if self.members.len() >= (self.max_members as usize).saturating_sub(1) {
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: false,
                        reason: JoinAckReason::Full,
                    });
                } else {
                    self.members.push((join_mac, join_pub_key));
                    actions.push(PairingAction::SendJoinAck {
                        host_mac: self.my_mac,
                        host_pub_key: self.my_pub_key,
                        join_mac,
                        accepted: true,
                        reason: JoinAckReason::Accepted,
                    });
                }
            }
            (
                IntercomState::Hosting(HostPhase::CollectingPeers),
                PairingEvent::BeaconTick,
            ) => {
                actions.push(PairingAction::SendBeacon {
                    host_mac: self.my_mac,
                    host_pub_key: self.my_pub_key,
                    mode: self.mode,
                    cur_members: self.members.len() as u8,
                    max_members: self.max_members,
                    joinable: self.members.len() < self.max_members as usize,
                });
            }
            (
                IntercomState::Hosting(HostPhase::CollectingPeers),
                PairingEvent::HostConfirm { switch_offset_ms, target_channel },
            ) => {
                self.switch_offset_ms = switch_offset_ms;
                self.target_channel = target_channel;
                self.directory_broadcasts_sent = 0;
                self.acks_received = 0;
                // D4.5: radio busy while DIRECTORY_BROADCAST + switch in flight.
                self.radio_busy = true;
                self.state = IntercomState::Hosting(HostPhase::Frozen);
                actions.push(PairingAction::ScheduleDirectoryBroadcasts);
                // Immediately send first broadcast (index 0).
                actions.push(PairingAction::SendDirectoryBroadcast {
                    member_count: (self.members.len() + 1) as u8,
                    mode: self.mode,
                    target_channel,
                    switch_offset_ms,
                    members: self.members.clone(),
                });
                self.directory_broadcasts_sent = 1;
            }
            (
                IntercomState::Hosting(HostPhase::Frozen),
                PairingEvent::DirectoryBroadcastTick { index },
            ) => {
                if index < 5 && self.directory_broadcasts_sent < 5 {
                    actions.push(PairingAction::SendDirectoryBroadcast {
                        member_count: (self.members.len() + 1) as u8,
                        mode: self.mode,
                        target_channel: self.target_channel,
                        switch_offset_ms: self.switch_offset_ms,
                        members: self.members.clone(),
                    });
                    self.directory_broadcasts_sent += 1;
                }
            }
            (
                IntercomState::Hosting(HostPhase::Frozen),
                PairingEvent::SwitchDeadline,
            ) => {
                // D5: clear_peers → set_channel → re-add peers with real pub_key.
                actions.push(PairingAction::ClearPeers);
                actions.push(PairingAction::SetChannel { channel: self.target_channel });
                for (mac, peer_pub) in &self.members {
                    // Caller derives LMK via CryptoService::derive_lmk(my_priv, peer_pub).
                    actions.push(PairingAction::AddPeer {
                        mac: *mac,
                        peer_pub_key: *peer_pub,
                    });
                }
                self.state = IntercomState::Hosting(HostPhase::SwitchingChannel);
            }
            (
                IntercomState::Hosting(HostPhase::SwitchingChannel),
                PairingEvent::ChannelSwitchAckReceived { status: _, .. },
            ) => {
                self.acks_received += 1;
                if self.acks_received >= self.members.len() as u8 {
                    actions.push(PairingAction::SaveGroup {
                        info: PairingGroupInfo {
                            my_priv_key: self.my_priv_key,
                            peers: self.members.clone(),
                            channel: self.target_channel,
                            mode: self.mode,
                        },
                    });
                    // D6.6: release radio_busy on entering Grouped.
                    self.radio_busy = false;
                    self.state = IntercomState::Grouped(VoiceState::Idle);
                    actions.push(PairingAction::EnterGrouped);
                }
            }
            (
                IntercomState::Hosting(HostPhase::SwitchingChannel),
                PairingEvent::AckTimeout,
            ) => {
                // D6: enter Grouped even if not all ACKs received.
                actions.push(PairingAction::SaveGroup {
                    info: PairingGroupInfo {
                        my_priv_key: self.my_priv_key,
                        peers: self.members.clone(),
                        channel: self.target_channel,
                        mode: self.mode,
                    },
                });
                self.radio_busy = false;
                self.state = IntercomState::Grouped(VoiceState::Idle);
                actions.push(PairingAction::EnterGrouped);
            }
            (
                IntercomState::Hosting(HostPhase::Discovering)
                | IntercomState::Hosting(HostPhase::CollectingPeers),
                PairingEvent::HostConfirmTimeout,
            ) => {
                // D7: 90s elapsed without confirm → Idle.
                self.state = IntercomState::Idle;
                actions.push(PairingAction::Fail(PairingFailure::HostTimeout));
            }
            (
                IntercomState::Hosting(HostPhase::Discovering)
                | IntercomState::Hosting(HostPhase::CollectingPeers),
                PairingEvent::Cancel,
            ) => {
                self.radio_busy = false;
                self.state = IntercomState::Idle;
            }

            // ---- Join: Searching ----
            (
                IntercomState::Joining(JoinPhase::Searching),
                PairingEvent::BeaconReceived {
                    host_mac,
                    host_pub_key,
                    mode,
                    cur_members,
                    max_members,
                    joinable,
                    rssi_4bar,
                },
            ) => {
                if !joinable {
                    // Host not accepting — skip.
                } else {
                    self.upsert_host(DiscoveredHost {
                        host_mac,
                        host_pub_key,
                        mode,
                        cur_members,
                        max_members,
                        joinable,
                        rssi_4bar,
                        last_seen_ms: 0,
                    });
                    actions.push(PairingAction::DiscoveredHostsUpdate);
                }
            }
            (
                IntercomState::Joining(JoinPhase::Searching),
                PairingEvent::SelectHost { host_mac },
            ) => {
                if let Some(h) = self.discovered_hosts.iter().find(|h| h.host_mac == host_mac) {
                    self.selected_host = Some(host_mac);
                    self.mode = h.mode;
                    actions.push(PairingAction::SendJoinReq {
                        join_mac: self.my_mac,
                        join_pub_key: self.my_pub_key,
                        host_mac,
                    });
                    self.state = IntercomState::Joining(JoinPhase::Requesting);
                } else {
                    actions.push(PairingAction::Fail(PairingFailure::HostEnded));
                }
            }
            (
                IntercomState::Joining(JoinPhase::Searching),
                PairingEvent::SearchTimeout,
            ) => {
                self.state = IntercomState::Idle;
                actions.push(PairingAction::Fail(PairingFailure::SearchTimeout));
            }

            // ---- Join: Requesting ----
            (
                IntercomState::Joining(JoinPhase::Requesting),
                PairingEvent::JoinAckReceived { accepted, reason, host_mac, host_pub_key },
            ) => {
                if accepted {
                    self.members.clear();
                    self.members.push((host_mac, host_pub_key));
                    self.state = IntercomState::Joining(JoinPhase::WaitingConfirm);
                } else {
                    self.state = IntercomState::Idle;
                    let fail = match reason {
                        JoinAckReason::Full => PairingFailure::GroupFull,
                        JoinAckReason::SchemaIncompatible => PairingFailure::SchemaIncompatible,
                        JoinAckReason::StateChanged => PairingFailure::StateChanged,
                        JoinAckReason::Accepted => PairingFailure::StateChanged, // unreachable
                    };
                    actions.push(PairingAction::Fail(fail));
                }
            }
            (
                IntercomState::Joining(JoinPhase::Requesting),
                PairingEvent::SearchTimeout,
            ) => {
                self.state = IntercomState::Idle;
                actions.push(PairingAction::Fail(PairingFailure::HostEnded));
            }

            // ---- Join: WaitingConfirm ----
            (
                IntercomState::Joining(JoinPhase::WaitingConfirm),
                PairingEvent::DirectoryBroadcastReceived {
                    member_count,
                    mode: _,
                    target_channel,
                    switch_offset_ms,
                    members,
                },
            ) => {
                self.target_channel = target_channel;
                self.members = members.clone();
                actions.push(PairingAction::ScheduleSwitch { offset_ms: switch_offset_ms });
                let _ = member_count;
                self.state = IntercomState::Joining(JoinPhase::SwitchingChannel);
            }
            (
                IntercomState::Joining(JoinPhase::SwitchingChannel),
                PairingEvent::SwitchDeadline,
            ) => {
                actions.push(PairingAction::ClearPeers);
                actions.push(PairingAction::SetChannel { channel: self.target_channel });
                for (mac, peer_pub) in &self.members {
                    actions.push(PairingAction::AddPeer {
                        mac: *mac,
                        peer_pub_key: *peer_pub,
                    });
                }
                actions.push(PairingAction::SendChannelSwitchAck { status: 0 });
                actions.push(PairingAction::SaveGroup {
                    info: PairingGroupInfo {
                        my_priv_key: self.my_priv_key,
                        peers: self.members.clone(),
                        channel: self.target_channel,
                        mode: self.mode,
                    },
                });
                self.radio_busy = false;
                self.state = IntercomState::Grouped(VoiceState::Idle);
                actions.push(PairingAction::EnterGrouped);
            }

            // ---- Cancel in early Join phases ----
            (
                IntercomState::Joining(JoinPhase::Searching)
                | IntercomState::Joining(JoinPhase::Requesting),
                PairingEvent::Cancel,
            ) => {
                self.radio_busy = false;
                self.state = IntercomState::Idle;
            }

            // ---- Default: ignore unmatched ----
            _ => {}
        }

        PairingOutcome {
            new_state: self.state.clone(),
            actions,
        }
    }
}

// ---- Trait shim for callers expecting a service-like object ---------------

pub trait PairingService: Send + Sync + fmt::Debug {
    fn start_discovery(&self) -> Result<(), PairingError>;
    fn current_state(&self) -> IntercomState;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    NotIdle,
    NoPeer,
    Timeout,
    Crypto,
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PairingError::NotIdle => write!(f, "not idle"),
            PairingError::NoPeer => write!(f, "no peer"),
            PairingError::Timeout => write!(f, "timeout"),
            PairingError::Crypto => write!(f, "crypto"),
        }
    }
}
impl std::error::Error for PairingError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> PairingMachine {
        PairingMachine::new([1, 2, 3, 4, 5, 6], [0xAA; 32])
    }

    #[test]
    fn host_path_basic() {
        let mut m = mk();
        // Start host.
        let o = m.handle(PairingEvent::StartHost {
            mode: IntercomMode::Clear,
            max_members: 4,
        });
        assert_eq!(o.new_state, IntercomState::Hosting(HostPhase::Discovering));

        // Beacon tick — emits SendBeacon.
        let o = m.handle(PairingEvent::BeaconTick);
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendBeacon { joinable: true, .. }
        )));

        // Join request received.
        let o = m.handle(PairingEvent::JoinReqReceived {
            join_mac: [10, 20, 30, 40, 50, 60],
            join_pub_key: [0xBB; 32],
            ver: BoardProfile::SCHEMA_VER,
        });
        assert_eq!(o.new_state, IntercomState::Hosting(HostPhase::CollectingPeers));
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendJoinAck { accepted: true, .. }
        )));
        assert_eq!(m.members().len(), 1);

        // Host confirm.
        let o = m.handle(PairingEvent::HostConfirm {
            switch_offset_ms: 500,
            target_channel: 11,
        });
        assert_eq!(o.new_state, IntercomState::Hosting(HostPhase::Frozen));
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::ScheduleDirectoryBroadcasts
        )));
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendDirectoryBroadcast { target_channel: 11, switch_offset_ms: 500, .. }
        )));

        // 4 more directory broadcast ticks (1 already sent at confirm time).
        for i in 1..5 {
            m.handle(PairingEvent::DirectoryBroadcastTick { index: i });
        }

        // Switch deadline.
        let o = m.handle(PairingEvent::SwitchDeadline);
        assert_eq!(o.new_state, IntercomState::Hosting(HostPhase::SwitchingChannel));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::ClearPeers)));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::SetChannel { channel: 11 })));
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::AddPeer { peer_pub_key: [0xBB; 32], .. }
        )));

        // ACK from join → enter Grouped.
        let o = m.handle(PairingEvent::ChannelSwitchAckReceived { sender_id: 0, status: 0 });
        assert_eq!(o.new_state, IntercomState::Grouped(VoiceState::Idle));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::SaveGroup { .. })));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::EnterGrouped)));
        assert!(!m.radio_busy());
    }

    #[test]
    fn join_path_basic() {
        let mut m = mk();
        m.handle(PairingEvent::StartJoin);
        assert_eq!(m.state(), &IntercomState::Joining(JoinPhase::Searching));

        // Beacon received.
        let o = m.handle(PairingEvent::BeaconReceived {
            host_mac: [10, 20, 30, 40, 50, 60],
            host_pub_key: [0xCC; 32],
            mode: IntercomMode::Clear,
            cur_members: 0,
            max_members: 4,
            joinable: true,
            rssi_4bar: 4,
        });
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::DiscoveredHostsUpdate)));
        assert_eq!(m.discovered_hosts().len(), 1);

        // Select host.
        let o = m.handle(PairingEvent::SelectHost { host_mac: [10, 20, 30, 40, 50, 60] });
        assert_eq!(o.new_state, IntercomState::Joining(JoinPhase::Requesting));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::SendJoinReq { .. })));

        // JoinAck accepted.
        let o = m.handle(PairingEvent::JoinAckReceived {
            host_mac: [10, 20, 30, 40, 50, 60],
            host_pub_key: [0xCC; 32],
            accepted: true,
            reason: JoinAckReason::Accepted,
        });
        assert_eq!(o.new_state, IntercomState::Joining(JoinPhase::WaitingConfirm));

        // DIRECTORY_BROADCAST.
        let o = m.handle(PairingEvent::DirectoryBroadcastReceived {
            member_count: 2,
            mode: IntercomMode::Clear,
            target_channel: 11,
            switch_offset_ms: 500,
            members: vec![([10, 20, 30, 40, 50, 60], [0xCC; 32])],
        });
        assert_eq!(o.new_state, IntercomState::Joining(JoinPhase::SwitchingChannel));

        // Switch deadline.
        let o = m.handle(PairingEvent::SwitchDeadline);
        assert_eq!(o.new_state, IntercomState::Grouped(VoiceState::Idle));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::SendChannelSwitchAck { status: 0 })));
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::EnterGrouped)));
    }

    #[test]
    fn join_rejected_full() {
        let mut m = mk();
        m.handle(PairingEvent::StartJoin);
        m.handle(PairingEvent::BeaconReceived {
            host_mac: [10; 6],
            host_pub_key: [0xCC; 32],
            mode: IntercomMode::Clear,
            cur_members: 3,
            max_members: 4,
            joinable: true,
            rssi_4bar: 4,
        });
        m.handle(PairingEvent::SelectHost { host_mac: [10; 6] });
        let o = m.handle(PairingEvent::JoinAckReceived {
            host_mac: [10; 6],
            host_pub_key: [0xCC; 32],
            accepted: false,
            reason: JoinAckReason::Full,
        });
        assert_eq!(o.new_state, IntercomState::Idle);
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::Fail(PairingFailure::GroupFull))));
    }

    #[test]
    fn search_timeout_fails() {
        let mut m = mk();
        m.handle(PairingEvent::StartJoin);
        let o = m.handle(PairingEvent::SearchTimeout);
        assert_eq!(o.new_state, IntercomState::Idle);
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::Fail(PairingFailure::SearchTimeout))));
    }

    #[test]
    fn host_confirm_timeout() {
        let mut m = mk();
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 4 });
        let o = m.handle(PairingEvent::HostConfirmTimeout);
        assert_eq!(o.new_state, IntercomState::Idle);
        assert!(o.actions.iter().any(|a| matches!(a, PairingAction::Fail(PairingFailure::HostTimeout))));
    }

    #[test]
    fn ack_timeout_still_enters_grouped() {
        let mut m = mk();
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 4 });
        m.handle(PairingEvent::JoinReqReceived {
            join_mac: [10; 6],
            join_pub_key: [0xBB; 32],
            ver: BoardProfile::SCHEMA_VER,
        });
        m.handle(PairingEvent::HostConfirm { switch_offset_ms: 500, target_channel: 11 });
        m.handle(PairingEvent::SwitchDeadline);
        let o = m.handle(PairingEvent::AckTimeout);
        assert_eq!(o.new_state, IntercomState::Grouped(VoiceState::Idle));
    }

    #[test]
    fn cancel_in_discovering() {
        let mut m = mk();
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 4 });
        let o = m.handle(PairingEvent::Cancel);
        assert_eq!(o.new_state, IntercomState::Idle);
    }

    #[test]
    fn discovered_host_dedup_updates() {
        let mut m = mk();
        m.handle(PairingEvent::StartJoin);
        m.handle(PairingEvent::BeaconReceived {
            host_mac: [10; 6],
            host_pub_key: [0xCC; 32],
            mode: IntercomMode::Clear,
            cur_members: 0,
            max_members: 4,
            joinable: true,
            rssi_4bar: 3,
        });
        m.handle(PairingEvent::BeaconReceived {
            host_mac: [10; 6],
            host_pub_key: [0xCC; 32],
            mode: IntercomMode::Clear,
            cur_members: 1,
            max_members: 4,
            joinable: true,
            rssi_4bar: 4,
        });
        assert_eq!(m.discovered_hosts().len(), 1);
        assert_eq!(m.discovered_hosts()[0].cur_members, 1);
        assert_eq!(m.discovered_hosts()[0].rssi_4bar, 4);
    }

    #[test]
    fn non_joinable_beacon_ignored() {
        let mut m = mk();
        m.handle(PairingEvent::StartJoin);
        m.handle(PairingEvent::BeaconReceived {
            host_mac: [10; 6],
            host_pub_key: [0xCC; 32],
            mode: IntercomMode::Clear,
            cur_members: 4,
            max_members: 4,
            joinable: false,
            rssi_4bar: 4,
        });
        assert_eq!(m.discovered_hosts().len(), 0);
    }

    #[test]
    fn host_full_rejects_with_reason() {
        let mut m = PairingMachine::new([1; 6], [0xAA; 32]);
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 2 });
        // First join accepted (members = 1, max-1 = 1).
        m.handle(PairingEvent::JoinReqReceived {
            join_mac: [10; 6],
            join_pub_key: [0xBB; 32],
            ver: BoardProfile::SCHEMA_VER,
        });
        // Second join rejected (max_members=2 means 1 peer + 1 host).
        let o = m.handle(PairingEvent::JoinReqReceived {
            join_mac: [20; 6],
            join_pub_key: [0xCC; 32],
            ver: BoardProfile::SCHEMA_VER,
        });
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendJoinAck { accepted: false, reason: JoinAckReason::Full, .. }
        )));
    }

    #[test]
    fn schema_mismatch_rejects_with_incompatible() {
        let mut m = mk();
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 4 });
        let o = m.handle(PairingEvent::JoinReqReceived {
            join_mac: [10; 6],
            join_pub_key: [0xBB; 32],
            ver: 99, // mismatched schema version
        });
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendJoinAck {
                accepted: false, reason: JoinAckReason::SchemaIncompatible, ..
            }
        )));
    }

    #[test]
    fn invalid_pubkey_rejects() {
        let mut m = mk();
        m.handle(PairingEvent::StartHost { mode: IntercomMode::Clear, max_members: 4 });
        let o = m.handle(PairingEvent::JoinReqReceived {
            join_mac: [10; 6],
            join_pub_key: [0x00; 32], // all-zero → invalid
            ver: BoardProfile::SCHEMA_VER,
        });
        assert!(o.actions.iter().any(|a| matches!(
            a,
            PairingAction::SendJoinAck {
                accepted: false, reason: JoinAckReason::SchemaIncompatible, ..
            }
        )));
    }

    #[test]
    fn host_info_from_discovered() {
        let h = DiscoveredHost {
            host_mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            host_pub_key: [0x11; 32],
            mode: IntercomMode::Clear,
            cur_members: 1,
            max_members: 4,
            joinable: true,
            rssi_4bar: 3,
            last_seen_ms: 1234,
        };
        let info = HostInfo::from_discovered(&h);
        assert_eq!(info.mac_suffix4, "eeff");
        assert_eq!(info.name, "Host-eeff");
        assert_eq!(info.rssi_4bar, 3);
        assert!(info.joinable);
    }

    #[test]
    fn format_mac_suffix4_lower_hex() {
        assert_eq!(format_mac_suffix4(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]), "eeff");
        assert_eq!(format_mac_suffix4(&[0, 0, 0, 0, 0x12, 0x34]), "1234");
    }

    #[test]
    fn valid_pubkey_check() {
        assert!(!is_valid_pub_key(&[0u8; 32]));
        assert!(!is_valid_pub_key(&[0xFFu8; 32]));
        assert!(is_valid_pub_key(&[0xAAu8; 32]));
    }
}
