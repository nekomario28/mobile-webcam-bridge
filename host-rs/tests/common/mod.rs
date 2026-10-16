//! Shared phone-side TLS fixtures for integration tests.
use super::wire;
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use std::io::{Read, Write};
use std::{net::TcpStream, sync::Arc, time::Duration};

pub const PHONE_SHA256: &str = "5331e886b4b4a360fb7ae16a0afc15791c595354a3c9bf329bf8b4a3eef25a25";
pub const ACCESS_KEY: &str = "00112233445566778899aabbccddeeff";

/// Accept one connection and start the TLS server side with the fixture phone
/// identity. Test-only fixture identity; never ship private fixtures as real
/// device identities.
/// Upgrade an already-accepted socket to the fixture phone's TLS server side.
pub fn phone_tls(socket: TcpStream) -> StreamOwned<ServerConnection, TcpStream> {
    socket
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let cert = CertificateDer::from(include_bytes!("../fixtures/test-phone-cert.der").to_vec());
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        include_bytes!("../fixtures/test-phone-key.der").to_vec(),
    ));
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
    StreamOwned::new(ServerConnection::new(Arc::new(config)).unwrap(), socket)
}

pub fn phone_binding() -> Vec<u8> {
    ring::digest::digest(
        &ring::digest::SHA256,
        include_bytes!("../fixtures/test-phone-cert.der"),
    )
    .as_ref()
    .to_vec()
}

/// Explicit test-phone approval after verifying the new client's proof. This
/// exercises the worker gate, not Android's approval surface or persistence.
#[allow(dead_code)] // The negative TLS-only suite intentionally never approves.
pub fn authorize_tls(stream: &mut StreamOwned<ServerConnection, TcpStream>, grant: bool) {
    use ring::rand::SecureRandom;
    let mut nonce = [0u8; wire::NONCE_BYTES];
    ring::rand::SystemRandom::new().fill(&mut nonce).unwrap();
    let challenge = wire::Frame::new(
        wire::CLIENT_CHALLENGE,
        0,
        wire::ClientChallenge {
            version: wire::CLIENT_AUTH_VERSION,
            transport: wire::AUTH_LAN,
            phone_binding: phone_binding(),
            nonce: nonce.to_vec(),
        }
        .encode(),
    )
    .unwrap();
    stream
        .write_all(&challenge.header.encode().unwrap())
        .unwrap();
    stream.write_all(&challenge.payload).unwrap();
    stream.flush().unwrap();
    let mut header = [0u8; wire::HEADER_SIZE];
    stream.read_exact(&mut header).unwrap();
    let header = wire::Header::decode(&header).unwrap();
    assert_eq!(header.kind, wire::CLIENT_AUTH);
    let mut payload = vec![0; header.payload_len as usize];
    stream.read_exact(&mut payload).unwrap();
    let proof = wire::ClientAuth::decode(&payload).unwrap();
    let message = wire::client_auth_message(
        wire::AUTH_LAN,
        &phone_binding(),
        &nonce,
        &proof.public_key,
        &proof.label,
    )
    .unwrap();
    ring::signature::UnparsedPublicKey::new(
        &ring::signature::ECDSA_P256_SHA256_FIXED,
        &proof.public_key,
    )
    .verify(&message, &proof.signature)
    .expect("worker supplied a valid v2 proof");
    let authorized = wire::Frame::new(
        wire::CLIENT_AUTH_RESULT,
        0,
        wire::ClientAuthResult {
            status: if grant {
                wire::AUTH_AUTHORIZED
            } else {
                wire::AUTH_PENDING
            },
            reason: wire::AUTH_REASON_NONE,
            expires_at_unix_seconds: if grant { 1_900_000_000 } else { 0 },
        }
        .encode(),
    )
    .unwrap();
    stream
        .write_all(&authorized.header.encode().unwrap())
        .unwrap();
    stream.write_all(&authorized.payload).unwrap();
    stream.flush().unwrap();
}
