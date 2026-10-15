//! This PC's own identity, for the per-PC authorization in
//! [per-PC authorization](../../docs/per-pc-authorization.md).
//!
//! The pairing key is a bearer token: whoever holds it connects. A PC identity
//! is the opposite — the phone stores the *public* half and can revoke one PC
//! without touching the others. This module owns the private half.
//!
//! Signing uses `ring`, which is already in the dependency tree through rustls
//! and is the only crypto this project trusts. Adding it as a direct
//! dependency introduces no new crate, no new version and no new audit surface;
//! rustls exposes no public way to *generate* a key, which is the one operation
//! this module needs.

use ring::signature::{
    ECDSA_P256_SHA256_FIXED, EcdsaKeyPair, EcdsaSigningAlgorithm, EcdsaVerificationAlgorithm,
    KeyPair,
};
use sha2::{Digest, Sha256};

/// The client ID is a truncated hash of the public key: short enough to display,
/// and the width the `CLIENT_AUTH` wire type reserves.
pub const CLIENT_ID_BYTES: usize = 16;

/// An uncompressed P-256 point: 0x04, then X and Y.
pub const PUBLIC_KEY_BYTES: usize = 65;

/// P-256 with SHA-256, for signing and for verification.
static SIGNING_ALG: &EcdsaSigningAlgorithm = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
static VERIFY_ALG: &EcdsaVerificationAlgorithm = &ECDSA_P256_SHA256_FIXED;

/// P-256 signatures are 64 raw bytes, which is what `CLIENT_AUTH` carries.
pub const SIGNATURE_BYTES: usize = 64;

/// One PC's identity. The private key is held as DER and never leaves as bytes
/// except through this module.
#[derive(Clone)]
pub struct Identity {
    private_key_der: Vec<u8>,
    public_key: Vec<u8>,
    client_id: [u8; CLIENT_ID_BYTES],
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("client_id", &hex(&self.client_id))
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Load the stored identity, or create and persist a new one.
    ///
    /// An unreadable or corrupt stored value is replaced rather than fatal: the
    /// phone will see an unknown client ID and ask again, which is the correct
    /// outcome for a key the user cannot recover anyway.
    pub fn load_or_create(
        read: impl FnOnce() -> crate::Result<Option<String>>,
        write: impl FnOnce(&str) -> crate::Result<()>,
    ) -> crate::Result<Self> {
        if let Some(stored) = read()?
            && let Some(identity) = Self::from_hex(&stored)
        {
            return Ok(identity);
        }
        let identity = Self::generate();
        write(&identity.to_hex())?;
        Ok(identity)
    }

    pub fn generate() -> Self {
        let rng = ring::rand::SystemRandom::new();
        let document = EcdsaKeyPair::generate_pkcs8(SIGNING_ALG, &rng)
            .expect("the platform provides P-256 key generation");
        Self::from_der(document.as_ref()).expect("a freshly generated key parses")
    }

    pub fn from_hex(value: &str) -> Option<Self> {
        let bytes = decode_hex(value)?;
        Self::from_der(&bytes)
    }

    pub fn to_hex(&self) -> String {
        hex(&self.private_key_der)
    }

    pub fn from_der(der: &[u8]) -> Option<Self> {
        let key =
            EcdsaKeyPair::from_pkcs8(SIGNING_ALG, der, &ring::rand::SystemRandom::new()).ok()?;
        // The raw uncompressed point is what the phone stores, so the public
        // half is that rather than an SPKI wrapper.
        let public_key = key.public_key().as_ref().to_vec();
        let client_id = client_id_of(&public_key);
        Some(Self {
            private_key_der: der.to_vec(),
            public_key,
            client_id,
        })
    }

    /// The stable public identifier, as the wire type carries it.
    pub fn client_id(&self) -> [u8; CLIENT_ID_BYTES] {
        self.client_id
    }

    /// The uncompressed public key point. The phone stores this; it is the
    /// public half and can be published freely.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Sign `message`. The caller supplies it; the wire contract covers
    /// `clientId || nonce || label`.
    pub fn sign(&self, message: &[u8]) -> crate::Result<[u8; SIGNATURE_BYTES]> {
        let key = EcdsaKeyPair::from_pkcs8(
            SIGNING_ALG,
            &self.private_key_der,
            &ring::rand::SystemRandom::new(),
        )
        .map_err(|_| "this PC's identity key is unusable")?;
        let signature = key
            .sign(&ring::rand::SystemRandom::new(), message)
            .map_err(|_| "signing failed")?;
        let bytes: [u8; SIGNATURE_BYTES] = signature
            .as_ref()
            .try_into()
            .map_err(|_| "unexpected signature length")?;
        Ok(bytes)
    }

    /// Verify a signature against a stored public key point. The phone side does
    /// this; it is here so the tests can prove the round trip.
    pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        ring::signature::UnparsedPublicKey::new(VERIFY_ALG, public_key)
            .verify(message, signature)
            .is_ok()
    }
}

/// The identifier is derived, never chosen: a truncated hash of the public key.
pub fn client_id_of(public_key: &[u8]) -> [u8; CLIENT_ID_BYTES] {
    Sha256::digest(public_key)[..CLIENT_ID_BYTES]
        .try_into()
        .expect("a SHA-256 digest is at least 16 bytes")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len() / 2)
        .map(|index| u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Vec<u8> {
        // The wire contract's own concatenation.
        let identity = Identity::generate();
        let mut message = identity.client_id().to_vec();
        message.extend_from_slice(&[7u8; 32]);
        message.extend_from_slice(b"studio-pc");
        message
    }

    #[test]
    fn a_generated_identity_signs_and_verifies() {
        let identity = Identity::generate();
        let message = message();
        let signature = identity.sign(&message).unwrap();
        assert!(Identity::verify(
            identity.public_key(),
            &message,
            &signature
        ));
    }

    #[test]
    fn a_changed_message_fails_verification() {
        let identity = Identity::generate();
        let mut message = message();
        let signature = identity.sign(&message).unwrap();
        message[0] ^= 1;
        assert!(!Identity::verify(
            identity.public_key(),
            &message,
            &signature
        ));
    }

    #[test]
    fn a_relabelled_message_fails_verification() {
        let identity = Identity::generate();
        let mut message = message();
        let signature = identity.sign(&message).unwrap();
        // The label is the last field, so changing it changes the signed bytes.
        let length = message.len();
        message[length - 1] = b'X';
        assert!(!Identity::verify(
            identity.public_key(),
            &message,
            &signature
        ));
    }

    #[test]
    fn another_pcs_key_cannot_forge_the_signature() {
        let mine = Identity::generate();
        let theirs = Identity::generate();
        let message = message();
        let signature = mine.sign(&message).unwrap();
        assert!(!Identity::verify(theirs.public_key(), &message, &signature));
    }

    #[test]
    fn the_identifier_survives_a_save_and_reload() {
        let identity = Identity::generate();
        let reloaded = Identity::from_hex(&identity.to_hex()).expect("round trips");
        assert_eq!(identity.client_id(), reloaded.client_id());
        assert_eq!(identity.public_key(), reloaded.public_key());
        let message = message();
        assert!(Identity::verify(
            reloaded.public_key(),
            &message,
            &identity.sign(&message).unwrap()
        ));
    }

    #[test]
    fn a_corrupt_stored_identity_is_replaced_not_fatal() {
        // The read and write closures cannot both hold the cell, so the store
        // is a channel between them rather than shared state.
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Some("not hex".to_string())).unwrap();
        let identity = Identity::load_or_create(
            || Ok(receiver.recv().ok().flatten()),
            |value| {
                sender.send(Some(value.to_string())).unwrap();
                Ok(())
            },
        )
        .expect("a corrupt identity is replaced");
        assert_eq!(
            receiver.recv().unwrap(),
            Some(identity.to_hex()),
            "the replacement was persisted"
        );
    }

    #[test]
    fn the_client_id_is_derived_from_the_public_key() {
        let identity = Identity::generate();
        assert_eq!(identity.public_key().len(), PUBLIC_KEY_BYTES);
        assert_eq!(identity.client_id(), client_id_of(identity.public_key()));
        assert_ne!(
            identity.client_id(),
            client_id_of(Identity::generate().public_key())
        );
    }

    #[test]
    fn the_signature_fills_the_wire_field() {
        let identity = Identity::generate();
        assert_eq!(identity.sign(&message()).unwrap().len(), SIGNATURE_BYTES);
        assert_eq!(SIGNATURE_BYTES, crate::wire::SIGNATURE_BYTES);
        assert_eq!(CLIENT_ID_BYTES, crate::wire::CLIENT_ID_BYTES);
    }

    /// The store hands back a string; a round trip through it must be exact.
    #[test]
    fn hex_round_trips_both_ways() {
        let identity = Identity::generate();
        let text = identity.to_hex();
        assert_eq!(text.len() % 2, 0);
        assert_eq!(
            Identity::from_hex(&text).unwrap().client_id(),
            identity.client_id()
        );
        assert!(Identity::from_hex(&format!(" {text} ")).is_some(), "trims");
        assert!(Identity::from_hex("abc").is_none(), "odd length");
        assert!(Identity::from_hex("zz").is_none(), "not hex");
        assert!(Identity::from_hex("").is_none(), "empty");
    }

    /// A signature is not a constant: the same message signed twice differs,
    /// because ECDSA is randomized, and both verify.
    #[test]
    fn the_same_message_signs_differently_and_both_verify() {
        let identity = Identity::generate();
        let message = message();
        let first = identity.sign(&message).unwrap();
        let second = identity.sign(&message).unwrap();
        assert_ne!(first, second, "ECDSA must not be deterministic");
        assert!(Identity::verify(identity.public_key(), &message, &first));
        assert!(Identity::verify(identity.public_key(), &message, &second));
    }
}
