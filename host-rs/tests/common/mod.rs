//! Shared phone-side TLS fixtures for integration tests.
use rustls::{
    ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
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
