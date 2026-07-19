//! Audio thread: all blocking I2S I/O + codec + jitter mixing, off the main
//! loop. change: wire-audio-pipeline (tasks 2.1-2.5). design §音频线程.
//!
//! Concurrency model: single main thread + this one audio thread (design
//! decision). The main thread owns UI + network-event bridging + pairing; the
//! audio thread owns the `Arc<HalAudioService>` and performs I2S capture /
//! playback so a blocking `read` never stalls the UI.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::intercom::codec::IntercomCodec;
use crate::intercom::jitter::JitterMixer;
use crate::services::audio_service::{AudioService, HalAudioService};

/// Max buffered inbound voice packets before drop-oldest (~320ms @ 20ms).
pub const VOICE_RX_CAP: usize = 16;

/// A wire voice packet queued for the audio thread (network recv → audio).
#[derive(Debug, Clone)]
pub struct VoicePacket {
    pub sender_id: u8,
    pub seq: u16,
    pub payload: Vec<u8>,
}

/// Cross-thread inbound voice queue (network recv → audio thread, task 2.3).
pub type VoiceRxQueue = Arc<Mutex<VecDeque<VoicePacket>>>;

/// Construct an empty bounded inbound voice queue.
pub fn new_voice_rx_queue() -> VoiceRxQueue {
    Arc::new(Mutex::new(VecDeque::with_capacity(VOICE_RX_CAP)))
}

/// Push a voice packet, dropping the oldest when full. Never blocks the
/// network recv path (mirrors the network-event queue drop-on-full policy).
pub fn push_voice_packet(q: &VoiceRxQueue, vp: VoicePacket) {
    if let Ok(mut dq) = q.lock() {
        if dq.len() >= VOICE_RX_CAP {
            dq.pop_front();
        }
        dq.push_back(vp);
    }
}

/// Encoded-frame sink: invoked per captured+encoded frame with `(seq, payload)`.
/// `wire-ptt-end-to-end` wires this to `network_svc.send`. Kept as a closure so
/// this change stays decoupled from group identity / send routing.
pub type TxSink = Box<dyn Fn(u16, &[u8]) + Send>;

/// Owns the spawned audio thread handle.
pub struct AudioThread;

impl AudioThread {
    /// Spawn the audio thread. Takes an `Arc<HalAudioService>` clone plus the
    /// inbound voice queue and TX sink. Loop:
    /// - TX: when capturing, `capture_frame` → `codec.encode` → `tx_sink`
    ///   (or self-loopback when `loopback`).
    /// - RX: drain `rx_q` → `codec.decode` → `JitterMixer` → `submit_pcm`
    ///   (which mixes ≤3 active routes + writes I2S).
    pub fn spawn(
        audio_svc: Arc<HalAudioService>,
        rx_q: VoiceRxQueue,
        tx_sink: TxSink,
        opus_enabled: bool,
        loopback: bool,
    ) -> std::io::Result<JoinHandle<()>> {
        thread::Builder::new()
            .name("audio".into())
            .stack_size(8 * 1024)
            .spawn(move || {
                let mut codec = IntercomCodec::new(opus_enabled);
                let mut mixer = JitterMixer::new();
                let mut tx_seq: u16 = 0;

                loop {
                    let mut did_work = false;

                    // ---- TX: capture → encode → send (or loopback) ----
                    if audio_svc.is_capturing() {
                        if let Ok(pcm) = audio_svc.capture_frame() {
                            let payload = codec.encode(&pcm);
                            tx_seq = tx_seq.wrapping_add(1);
                            if loopback {
                                // Self-test (design §自检 loopback): decode +
                                // play back own capture, no packet sent.
                                let back = codec.decode(&payload);
                                let _ = audio_svc.submit_pcm(0, &back);
                            } else {
                                tx_sink(tx_seq, &payload);
                            }
                            did_work = true;
                        }
                    }

                    // ---- RX: drain inbound voice → decode → mixer ----
                    let drained: Vec<VoicePacket> = match rx_q.lock() {
                        Ok(mut dq) => dq.drain(..).collect(),
                        Err(_) => Vec::new(),
                    };
                    for vp in drained {
                        let pcm = codec.decode(&vp.payload);
                        mixer.submit_pcm(vp.sender_id as usize, pcm);
                        did_work = true;
                    }

                    // Mix active routes (≤ MIX_MAX_ROUTES) and write to I2S.
                    // submit_pcm performs the attenuation/sum/limit + I2S write.
                    for (sid, pcm) in mixer.active_routes() {
                        let _ = audio_svc.submit_pcm(sid as u8, &pcm);
                        did_work = true;
                    }

                    if !did_work {
                        // Idle: avoid busy-spin. When capturing, the blocking
                        // I2S read paces the loop instead.
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            })
    }
}
