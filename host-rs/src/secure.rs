//! Authenticated, encrypted Wi-Fi transport.
//! TLS 1.3 uses a fingerprint pinned from the phone screen, then presents a
//! 128-bit random per-installation credential inside the encrypted channel.
//! No code path attempts plaintext after a handshake failure.
use crate::transport::{self, ByteStream, SocketTransport};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use std::{net::TcpStream, sync::Arc, time::Duration};

#[derive(Debug)]
struct PinnedCertificate {
    sha256: [u8; 32],
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        certificate: &CertificateDer<'_>,
        _chain: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let found = Sha256::digest(certificate.as_ref());
        if found[..] != self.sha256[..] {
            return Err(Error::General(
                "Wi-Fi phone certificate fingerprint mismatch".into(),
            ));
        }
        // The actual TLS handshake signature is independently verified below.
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// A pairing credential is exactly `bytes * 2` ASCII hex digits.
pub fn is_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes * 2 && value.bytes().all(|c| c.is_ascii_hexdigit())
}

pub fn parse_hex(value: &str, bytes: usize) -> crate::Result<Vec<u8>> {
    if !is_hex(value, bytes) {
        return Err("Wi-Fi pairing fingerprint or key is missing/invalid".into());
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

pub struct SecureTcp(StreamOwned<ClientConnection, TcpStream>);

impl SocketTransport for SecureTcp {
    fn socket(&mut self) -> &mut TcpStream {
        &mut self.0.sock
    }
    fn stream(&mut self) -> &mut dyn ByteStream {
        &mut self.0
    }
}

impl SecureTcp {
    pub fn connect(
        host: &str,
        port: u16,
        fingerprint: &str,
        key: &str,
        timeout: Duration,
    ) -> crate::Result<Self> {
        let pin: [u8; 32] = parse_hex(fingerprint, 32)?
            .try_into()
            .map_err(|_| "invalid phone certificate fingerprint")?;
        let credential = parse_hex(key, 16)?;

        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PinnedCertificate {
                sha256: pin,
                provider,
            }))
            .with_no_client_auth();
        // Identity is established by the pinned certificate, not by DNS.
        let server_name = ServerName::try_from("mobile-webcam.local")?.to_owned();
        transport::dial(host, port, timeout, |socket, deadline| {
            socket.set_nodelay(true)?;
            let client = ClientConnection::new(Arc::new(config.clone()), server_name.clone())?;
            let mut secure = Self(StreamOwned::new(client, socket));
            while secure.0.conn.is_handshaking() {
                let left = transport::remaining(deadline)?;
                secure.0.sock.set_read_timeout(Some(left))?;
                secure.0.sock.set_write_timeout(Some(left))?;
                secure.0.conn.complete_io(&mut secure.0.sock)?;
            }
            secure.pair(credential.clone(), transport::remaining(deadline)?)?;
            Ok(secure)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_keys_have_fixed_binary_length() {
        assert_eq!(
            parse_hex("00112233445566778899aabbccddeeff", 16)
                .unwrap()
                .len(),
            16
        );
        assert!(parse_hex("000000", 16).is_err());
        assert!(parse_hex("00112233445566778899AABBCCDDEEFG", 16).is_err());
    }
    #[test]
    fn incorrect_certificate_is_rejected_before_video() {
        let cert = CertificateDer::from(vec![1u8, 2, 3, 4]);
        let pin = PinnedCertificate {
            sha256: [0; 32],
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        };
        let name = ServerName::try_from("mobile-webcam.local").unwrap();
        assert!(
            pin.verify_server_cert(&cert, &[], &name, &[], UnixTime::now())
                .is_err()
        );
    }
}
