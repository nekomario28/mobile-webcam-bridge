//! Authenticated, encrypted Wi-Fi transport.
//! TLS 1.3 uses a fingerprint pinned from the phone screen, then presents a
//! 128-bit random per-installation credential inside the encrypted channel.
//! No code path attempts plaintext after a handshake failure.
use crate::{
    transport::Transport,
    wire::{self, Frame, Header},
};
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, Error, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::Arc,
    time::{Duration, Instant},
};

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
        if found.as_slice() != self.sha256 {
            return Err(Error::General("Wi-Fi phone certificate fingerprint mismatch".into()));
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

pub struct SecureTcp(StreamOwned<ClientConnection, TcpStream>);

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "TLS deadline exceeded"))
}

pub fn parse_hex(value: &str, bytes: usize) -> crate::Result<Vec<u8>> {
    if value.len() != bytes * 2 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Wi-Fi pairing fingerprint or key is missing/invalid".into());
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
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
        if host.trim().is_empty() {
            return Err("enter a phone IP address".into());
        }

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
        let addresses: Vec<SocketAddr> = match host.trim().parse::<SocketAddr>() {
            Ok(addr) => vec![addr],
            Err(_) => (host.trim(), port).to_socket_addrs()?.collect(),
        };
        let deadline = Instant::now() + timeout;
        let mut last: Box<dyn std::error::Error + Send + Sync> = "no phone address".into();
        for address in addresses {
            let raw = match TcpStream::connect_timeout(&address, remaining(deadline)?) {
                Ok(s) => s,
                Err(e) => { last = Box::new(e); continue; }
            };
            raw.set_nodelay(true)?;
            let client = ClientConnection::new(Arc::new(config.clone()), server_name.clone())?;
            let mut tls = StreamOwned::new(client, raw);
            let attempt = (|| -> crate::Result<Self> {
                while tls.conn.is_handshaking() {
                    let left = remaining(deadline)?;
                    tls.sock.set_read_timeout(Some(left))?;
                    tls.sock.set_write_timeout(Some(left))?;
                    tls.conn.complete_io(&mut tls.sock)?;
                }
                let mut secure = Self(tls);
                secure.send(&Frame::new(wire::HELLO, 1, credential.clone())?, remaining(deadline)?)?;
                let ack = secure.receive(remaining(deadline)?)?;
                if ack.header.kind != wire::HELLO_ACK
                    || ack.header.sequence != 1
                    || !ack.payload.is_empty()
                {
                    return Err("Wi-Fi pairing was not authorized".into());
                }
                Ok(secure)
            })();
            match attempt {
                Ok(secure) => return Ok(secure),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn read_exact_deadline(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut offset = 0;
        while offset < bytes.len() {
            self.0.sock.set_read_timeout(Some(remaining(deadline)?))?;
            match self.0.read(&mut bytes[offset..]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "TLS peer closed")),
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl Transport for SecureTcp {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        let mut bytes = [0; wire::HEADER_SIZE];
        self.read_exact_deadline(&mut bytes, timeout)?;
        let header = Header::decode(&bytes)?;
        let mut payload = vec![0; header.payload_len as usize];
        self.read_exact_deadline(&mut payload, timeout)?;
        Ok(Frame { header, payload })
    }

    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        frame.validate()?;
        let deadline = Instant::now() + timeout;
        let header = frame.header.encode()?;
        for bytes in [header.as_slice(), frame.payload.as_slice()] {
            let mut offset = 0;
            while offset < bytes.len() {
                self.0.sock.set_write_timeout(Some(remaining(deadline)?))?;
                match self.0.write(&bytes[offset..]) {
                    Ok(0) => return Err("TLS write made no progress".into()),
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        self.0.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_keys_have_fixed_binary_length() {
        assert_eq!(parse_hex("00112233445566778899aabbccddeeff", 16).unwrap().len(), 16);
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
        assert!(pin.verify_server_cert(&cert, &[], &name, &[], UnixTime::now()).is_err());
    }
}
