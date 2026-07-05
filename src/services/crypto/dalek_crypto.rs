//! curve25519-dalek backed CryptoService. change 03/17.
//! design D4: x25519::StaticSecret + diffie_hellman. D5: HKDF-SHA256.
//! D6: validate_pubkey rejects all-zero.
//!
//! NB: x25519-dalek 2.x was split out of curve25519-dalek 4.x. design D4
//! mentions curve25519-dalek's x25519 module; that was removed in dalek 4.x
//! and the maintained successor is the standalone x25519-dalek crate (same
//! underlying curve, same RFC 7748). Recorded in change 03 Errata.

#![allow(dead_code)]

use std::fmt;

use esp_idf_svc::sys as esp_idf_sys;
use rand_core::{CryptoRng, Error as RngError, RngCore};
use x25519_dalek::{PublicKey, StaticSecret};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::services::crypto::{CryptoService, KeyPair};

/// HKDF-SHA256 salt and info (design D5, compile-time constants).
const HKDF_SALT: &[u8] = b"ESP32C6-INTERCOM";
const HKDF_INFO: &[u8] = b"LMK-v1";

/// RNG wrapper around ESP-IDF `esp_random()` (hardware RNG on ESP32-C6,
/// seeded from RF noise when radio is on). Used to feed
/// `StaticSecret::random_from_rng`.
struct EspRng;

impl RngCore for EspRng {
    fn next_u32(&mut self) -> u32 {
        // SAFETY: esp_random is thread-safe and panics only before RNG init
        // (which happens in bootloader, well before app_main).
        unsafe { esp_idf_sys::esp_random() }
    }
    fn next_u64(&mut self) -> u64 {
        let hi = self.next_u32() as u64;
        let lo = self.next_u32() as u64;
        (hi << 32) | lo
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut i = 0;
        while i + 4 <= dest.len() {
            let v = self.next_u32().to_le_bytes();
            dest[i..i + 4].copy_from_slice(&v);
            i += 4;
        }
        if i < dest.len() {
            let v = self.next_u32().to_le_bytes();
            let rem = dest.len() - i;
            dest[i..].copy_from_slice(&v[..rem]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), RngError> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl CryptoRng for EspRng {}

pub struct DalekCrypto;

impl fmt::Debug for DalekCrypto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DalekCrypto").finish_non_exhaustive()
    }
}

impl DalekCrypto {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DalekCrypto {
    fn default() -> Self {
        Self::new()
    }
}

impl CryptoService for DalekCrypto {
    fn gen_keypair(&self) -> KeyPair {
        let mut rng = EspRng;
        let secret = StaticSecret::random_from_rng(&mut rng);
        let pub_key = PublicKey::from(&secret);
        KeyPair {
            priv_key: secret.to_bytes(),
            pub_key: pub_key.to_bytes(),
        }
    }

    fn derive_lmk(&self, my_priv: &[u8; 32], peer_pub: &[u8; 32]) -> [u8; 16] {
        let secret = StaticSecret::from(*my_priv);
        let peer = PublicKey::from(*peer_pub);
        let shared = secret.diffie_hellman(&peer);
        let shared_bytes = shared.to_bytes();

        // HKDF-Extract+Expand, take first 16 bytes for ESP-NOW AES-CCM LMK.
        let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &shared_bytes);
        let mut okm = [0u8; 32];
        hk.expand(HKDF_INFO, &mut okm)
            .expect("HKDF expand on 32-byte output cannot fail");
        let mut lmk = [0u8; 16];
        lmk.copy_from_slice(&okm[..16]);
        lmk
    }

    fn validate_pubkey(&self, pub_key: &[u8; 32]) -> bool {
        // design D6: reject all-zero only (X25519 accepts any 32 bytes).
        pub_key != &[0u8; 32]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lmk_symmetric() {
        let cs = DalekCrypto::new();
        let a = cs.gen_keypair();
        let b = cs.gen_keypair();
        let lmk_ab = cs.derive_lmk(&a.priv_key, &b.pub_key);
        let lmk_ba = cs.derive_lmk(&b.priv_key, &a.pub_key);
        assert_eq!(lmk_ab, lmk_ba);
    }

    #[test]
    fn lmk_length_16() {
        let cs = DalekCrypto::new();
        let a = cs.gen_keypair();
        let b = cs.gen_keypair();
        let lmk = cs.derive_lmk(&a.priv_key, &b.pub_key);
        assert_eq!(lmk.len(), 16);
    }

    #[test]
    fn different_peers_different_lmk() {
        let cs = DalekCrypto::new();
        let me = cs.gen_keypair();
        let a = cs.gen_keypair();
        let b = cs.gen_keypair();
        let lmk_a = cs.derive_lmk(&me.priv_key, &a.pub_key);
        let lmk_b = cs.derive_lmk(&me.priv_key, &b.pub_key);
        assert_ne!(lmk_a, lmk_b);
    }

    #[test]
    fn reject_allzero_pubkey() {
        let cs = DalekCrypto::new();
        assert!(!cs.validate_pubkey(&[0u8; 32]));
    }

    #[test]
    fn accept_nonzero_pubkey() {
        let cs = DalekCrypto::new();
        let kp = cs.gen_keypair();
        assert!(cs.validate_pubkey(&kp.pub_key));
    }

    #[test]
    fn keypairs_unique() {
        let cs = DalekCrypto::new();
        let a = cs.gen_keypair();
        let b = cs.gen_keypair();
        assert_ne!(a.priv_key, b.priv_key);
    }
}
