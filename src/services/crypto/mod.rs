//! Crypto service: trait + DalekCrypto impl. change 03/17.
//! Reference: 技术设计 §3.2 / §6.1 / change 03 design D4-D6.

#![allow(dead_code)]

pub mod dalek_crypto;

pub use dalek_crypto::DalekCrypto;

use std::fmt::Debug;

/// X25519 keypair. §3.2.
#[derive(Debug, Clone, Copy)]
pub struct KeyPair {
    pub priv_key: [u8; 32],
    pub pub_key: [u8; 32],
}

/// Crypto service trait (§3.2). Send + Sync.
pub trait CryptoService: Send + Sync + Debug {
    fn gen_keypair(&self) -> KeyPair;
    fn derive_lmk(&self, my_priv: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 16];
    fn validate_pubkey(&self, pub_key: &[u8; 32]) -> bool;
}
