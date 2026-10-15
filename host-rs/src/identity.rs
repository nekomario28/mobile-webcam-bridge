//! This PC's own identity, for the per-PC authorization in
//! [per-PC authorization](../../docs/per-pc-authorization.md).
//!
//! The pairing key is a bearer token: whoever holds it connects. A PC identity
//! is the opposite — the phone stores the *public* half and can revoke one PC
//! without touching the others. This module owns the private half.
//!
//! Signing uses `ring`, already in the dependency tree through rustls. The
//! direct dependency adds no crate or version, but this use of its key-generation
//! and signing APIs still needs review independently of TLS.

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
    /// Only a missing value creates an identity. Read errors and corrupt values
    /// fail without writing: silently rotating an existing key would discard
    /// the PC's authorization. Explicit recovery belongs to the storage/UI
    /// boundary, which is not implemented here yet.
    pub fn load_or_create(
        read: impl FnOnce() -> crate::Result<Option<String>>,
        write: impl FnOnce(&str) -> crate::Result<()>,
    ) -> crate::Result<Self> {
        if let Some(stored) = read()? {
            return Self::from_hex(&stored).ok_or_else(|| {
                "this PC's stored identity is corrupt; explicit recovery required".into()
            });
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
    // Validate ASCII hex before byte slicing: even byte length alone does not
    // make an index a UTF-8 character boundary (for example "0é0").
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
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
    fn a_corrupt_stored_identity_is_not_silently_replaced() {
        for stored in ["not hex", "00", "0é0"] {
            let written = std::cell::Cell::new(false);
            let result = Identity::load_or_create(
                || Ok(Some(stored.to_string())),
                |_| {
                    written.set(true);
                    Ok(())
                },
            );
            assert!(result.is_err(), "corrupt identity must require recovery");
            assert!(!written.get(), "never overwrite an existing identity");
        }
    }

    #[test]
    fn missing_identity_is_created_but_read_errors_never_write() {
        let written = std::cell::RefCell::new(None);
        let generated = Identity::load_or_create(
            || Ok(None),
            |value| {
                *written.borrow_mut() = Some(value.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            written.borrow().as_deref(),
            Some(generated.to_hex().as_str())
        );
        let loaded = Identity::load_or_create(
            || Ok(written.borrow().clone()),
            |_| panic!("a valid stored identity must not be replaced"),
        )
        .unwrap();
        assert_eq!(loaded.client_id(), generated.client_id());

        let error = Identity::load_or_create(
            || Err("permission denied".into()),
            |_| panic!("a read error must not overwrite the store"),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "permission denied");
        assert!(Identity::load_or_create(|| Ok(None), |_| Err("disk full".into())).is_err());
    }

    #[test]
    fn malformed_identity_text_never_panics() {
        for input in ["0é0", "😀", "a€", "+a", "zz", "abc", ""] {
            assert!(Identity::from_hex(input).is_none(), "reject {input:?}");
        }
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

    /// The same fixture the Kotlin suite verifies. Both languages verifying one
    /// signature is what proves they agree on the scheme; a mismatch here is a
    /// wire-format bug, not a test bug.
    #[test]
    fn the_canonical_signature_verifies_in_both_languages() {
        verify_fixture(include_bytes!(
            "../../protocol/fixtures/client-signature-v1.bin"
        ));
    }

    #[test]
    fn a_leading_zero_signature_verifies_in_both_languages() {
        let fixture =
            include_bytes!("../../protocol/fixtures/client-signature-leading-zero-v1.bin");
        let signature = &fixture[PUBLIC_KEY_BYTES + 57..];
        assert_eq!(
            signature[0], 0,
            "r must exercise the old nonminimal DER bug"
        );
        assert!((1..0x80).contains(&signature[1]));
        verify_fixture(fixture);
    }

    fn verify_fixture(fixture: &[u8]) {
        let (public_key, rest) = fixture.split_at(PUBLIC_KEY_BYTES);
        let (message, signature) = rest.split_at(57);
        assert_eq!(signature.len(), SIGNATURE_BYTES);
        assert!(Identity::verify(public_key, message, signature));
        // The client ID inside the message is the derived one.
        assert_eq!(
            client_id_of(public_key).to_vec(),
            message[..CLIENT_ID_BYTES]
        );
        // One changed byte anywhere in the signed message fails.
        for index in [0, 16, 48, 56] {
            let mut tampered = message.to_vec();
            tampered[index] ^= 1;
            assert!(!Identity::verify(public_key, &tampered, signature));
        }
        // And the fixture's own sizes are what the wire reserves.
        assert_eq!(public_key.len(), PUBLIC_KEY_BYTES);
        assert_eq!(fixture.len(), PUBLIC_KEY_BYTES + 57 + SIGNATURE_BYTES);
    }

    /// ring uses randomized signing here. Deterministic ECDSA is also valid;
    /// this test pins this backend's behavior, not a cryptographic requirement.
    #[test]
    fn the_same_message_signs_differently_and_both_verify() {
        let identity = Identity::generate();
        let message = message();
        let first = identity.sign(&message).unwrap();
        let second = identity.sign(&message).unwrap();
        assert_ne!(first, second, "ring uses fresh signing randomness");
        assert!(Identity::verify(identity.public_key(), &message, &first));
        assert!(Identity::verify(identity.public_key(), &message, &second));
    }
}

#[cfg(test)]
mod fixture_generator {
    //! Run with `cargo test --lib identity::fixture_generator::print -- --ignored --nocapture`
    //! to regenerate protocol/fixtures/client-signature-v1.bin.
    use super::*;

    /// Produces a valid signature with redundant fixed-width leading padding.
    /// The private key is discarded; the committed fixture contains public data.
    #[test]
    #[ignore = "generator, not a test"]
    fn leading_zero() {
        let identity = Identity::generate();
        let mut message = identity.client_id().to_vec();
        message.extend_from_slice(&[7u8; 32]);
        message.extend_from_slice(b"studio-pc");
        let signature = (0..10_000)
            .find_map(|_| {
                let signature = identity.sign(&message).unwrap();
                (signature[0] == 0 && (1..0x80).contains(&signature[1])).then_some(signature)
            })
            .expect("no suitable signature within the bounded generation budget");
        let mut out = identity.public_key().to_vec();
        out.extend_from_slice(&message);
        out.extend_from_slice(&signature);
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../protocol/fixtures/client-signature-leading-zero-v1.bin"),
            &out,
        )
        .unwrap();
        println!("leading-zero signature fixture: {} public bytes", out.len());
    }

    #[test]
    #[ignore = "generator, not a test"]
    fn print() {
        let identity = Identity::generate();
        let client_id = identity.client_id();
        let nonce = [7u8; 32];
        let label = b"studio-pc";
        let mut message = client_id.to_vec();
        message.extend_from_slice(&nonce);
        message.extend_from_slice(label);
        let signature = identity.sign(&message).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(identity.public_key());
        out.extend_from_slice(&message);
        out.extend_from_slice(&signature);
        std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../protocol/fixtures/client-signature-v1.bin"),
            &out,
        )
        .unwrap();
        println!(
            "public {} message {} signature {} total {}",
            identity.public_key().len(),
            message.len(),
            signature.len(),
            out.len()
        );
        println!("client_id {}", hex(&client_id));
    }
}
