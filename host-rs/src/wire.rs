use std::io;

pub const HEADER_SIZE: usize = 24;
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
pub const HELLO: u8 = 0x01;
pub const HELLO_ACK: u8 = 0x02;
pub const PING: u8 = 0x03;
pub const PONG: u8 = 0x04;
// Quality control extension. Types stay in the control space; both
// languages read protocol/fixtures/quality-capabilities-v1.bin.
pub const QUALITY_CAPABILITIES: u8 = 0x05;
pub const PREPARE_QUALITY: u8 = 0x06;
pub const QUALITY_RESULT: u8 = 0x07;
pub const PAUSED: u8 = 0x08;
pub const COMMIT_QUALITY: u8 = 0x09;
/// Per-PC authorization (designed, not yet wired). The phone challenges, the PC
/// proves possession of its key, the phone answers.
pub const CLIENT_CHALLENGE: u8 = 0x0a;
pub const CLIENT_AUTH: u8 = 0x0b;
pub const CLIENT_AUTH_RESULT: u8 = 0x0c;
/// The phone hands the PC its Wi-Fi pairing details over the USB cable, so the
/// first Wi-Fi session needs nothing typed. User-initiated on the phone; the
/// question it answers is "what do I type", not "is this phone trusted".
pub const PAIRING_INFO: u8 = 0x0d;
/// "A host is here, start a fresh exchange."
///
/// USB has no session boundary: the phone sends the authorization challenge, and it sends
/// exactly one per accessory session, so a host that reconnects finds a session that has
/// already been used — quiet (if it authorised someone) or answering with control frames
/// from the older connection. Both look like a phone that "does not support authorization
/// v2". This frame is the host saying so out loud, so the phone can end that session and
/// issue a fresh challenge (protocol/client-authorization-v2.md). Zero payload: it carries
/// no identity and grants nothing; it is a reset signal, not a credential.
pub const HOST_HELLO: u8 = 0x0e;
pub const VIDEO_CONFIG: u8 = 0x10;
pub const VIDEO_AU: u8 = 0x11;
pub const IDR_REQUEST: u8 = 0x12;
pub const KEYFRAME: u16 = 1;
pub const CONFIG_INCLUDED: u16 = 2;
pub const DISCONTINUITY: u16 = 4;

/// Client authorization outcomes.
pub const AUTH_AUTHORIZED: u8 = 0;
pub const AUTH_PENDING: u8 = 1;
pub const AUTH_REJECTED: u8 = 2;
pub const AUTH_REASON_NONE: u8 = 0;
pub const AUTH_REASON_UNKNOWN: u8 = 1;
pub const AUTH_REASON_REVOKED: u8 = 2;
pub const AUTH_REASON_EXPIRED: u8 = 3;
pub const AUTH_REASON_TOO_MANY: u8 = 4;
pub const AUTH_REASON_BAD_SIGNATURE: u8 = 5;

/// A PC-proposed label. Attacker-chosen: never an identity.
pub const MAX_CLIENT_LABEL: usize = 32;
pub const CLIENT_ID_BYTES: usize = 16;
pub const SIGNATURE_BYTES: usize = 64;
pub const NONCE_BYTES: usize = 32;

/// Control payload limit; the counted capability list must fit inside it.
pub const MAX_CONTROL_PAYLOAD: usize = 16 * 1024;
pub const MAX_QUALITY_MODES: usize = 256;
pub const QUALITY_MODE_BYTES: usize = 12;

/// Camera permission / feature state advertised with the capability list.
pub const PERMISSION_READY: u8 = 0;
pub const PERMISSION_PENDING: u8 = 1;
pub const PERMISSION_UNAVAILABLE: u8 = 2;

/// PrepareQuality outcome states.
pub const RESULT_SWITCHING: u8 = 0;
pub const RESULT_REJECTED: u8 = 1;
pub const RESULT_APPLIED: u8 = 2;
pub const RESULT_STAGED: u8 = 3;
pub const REASON_NONE: u8 = 0;
pub const REASON_PERMISSION: u8 = 1;
pub const REASON_FOREGROUND: u8 = 2;
pub const REASON_UNSUPPORTED_MODE: u8 = 3;
pub const REASON_STALE_REVISION: u8 = 4;
pub const REASON_BUSY: u8 = 5;
pub const REASON_DEADLINE: u8 = 6;
pub const REASON_CAPTURE_FAILED: u8 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityMode {
    pub width: u16,
    pub height: u16,
    pub target_fps: u16,
    pub bitrate: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub extension_version: u8,
    pub permission: u8,
    pub catalog_revision: u16,
    pub stream_epoch: u32,
    pub modes: Vec<QualityMode>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PrepareQuality {
    pub request_id: u32,
    pub catalog_revision: u16,
    pub width: u16,
    pub height: u16,
    pub target_fps: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub struct QualityResult {
    pub request_id: u32,
    pub status: u8,
    pub reason: u8,
    pub applied_width: u16,
    pub applied_height: u16,
    pub applied_fps: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Epoch {
    pub request_id: u32,
    pub stream_epoch: u32,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

fn need(payload: &[u8], bytes: usize, message: &str) -> io::Result<()> {
    if payload.len() < bytes {
        return Err(invalid(message));
    }
    Ok(())
}

impl Capabilities {
    /// Fixed-little-endian fields, then a counted mode list. Length is
    /// validated against the count before anything is trusted.
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        need(payload, 10, "quality capabilities payload is truncated")?;
        let mode_count = u16::from_le_bytes(payload[8..10].try_into().unwrap()) as usize;
        if mode_count > MAX_QUALITY_MODES {
            return Err(invalid("quality capability list exceeds 256 modes"));
        }
        let expected = 10 + mode_count * QUALITY_MODE_BYTES;
        if payload.len() != expected {
            return Err(invalid(
                "quality capabilities length does not match the mode count",
            ));
        }
        if payload.len() > MAX_CONTROL_PAYLOAD {
            return Err(invalid(
                "quality capabilities payload exceeds the control limit",
            ));
        }
        let mut modes = Vec::with_capacity(mode_count);
        for chunk in payload[10..].as_chunks::<{ QUALITY_MODE_BYTES }>().0 {
            modes.push(QualityMode {
                width: u16::from_le_bytes(chunk[0..2].try_into().unwrap()),
                height: u16::from_le_bytes(chunk[2..4].try_into().unwrap()),
                target_fps: u16::from_le_bytes(chunk[4..6].try_into().unwrap()),
                bitrate: u32::from_le_bytes(chunk[8..12].try_into().unwrap()),
            });
        }
        Ok(Self {
            extension_version: payload[0],
            permission: payload[1],
            catalog_revision: u16::from_le_bytes(payload[2..4].try_into().unwrap()),
            stream_epoch: u32::from_le_bytes(payload[4..8].try_into().unwrap()),
            modes,
        })
    }

    pub fn encode(&self) -> io::Result<Vec<u8>> {
        if self.modes.len() > MAX_QUALITY_MODES {
            return Err(invalid("quality capability list exceeds 256 modes"));
        }
        let mut payload = Vec::with_capacity(10 + self.modes.len() * QUALITY_MODE_BYTES);
        payload.push(self.extension_version);
        payload.push(self.permission);
        payload.extend_from_slice(&self.catalog_revision.to_le_bytes());
        payload.extend_from_slice(&self.stream_epoch.to_le_bytes());
        payload.extend_from_slice(&(self.modes.len() as u16).to_le_bytes());
        for mode in &self.modes {
            payload.extend_from_slice(&mode.width.to_le_bytes());
            payload.extend_from_slice(&mode.height.to_le_bytes());
            payload.extend_from_slice(&mode.target_fps.to_le_bytes());
            payload.extend_from_slice(&0u16.to_le_bytes());
            payload.extend_from_slice(&mode.bitrate.to_le_bytes());
        }
        if payload.len() > MAX_CONTROL_PAYLOAD {
            return Err(invalid(
                "quality capabilities payload exceeds the control limit",
            ));
        }
        Ok(payload)
    }
}

impl PrepareQuality {
    pub const BYTES: usize = 14;
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if payload.len() != Self::BYTES {
            return Err(invalid("prepare quality payload must be 14 bytes"));
        }
        Ok(Self {
            request_id: u32::from_le_bytes(payload[0..4].try_into().unwrap()),
            catalog_revision: u16::from_le_bytes(payload[4..6].try_into().unwrap()),
            width: u16::from_le_bytes(payload[6..8].try_into().unwrap()),
            height: u16::from_le_bytes(payload[8..10].try_into().unwrap()),
            target_fps: u16::from_le_bytes(payload[10..12].try_into().unwrap()),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(Self::BYTES);
        payload.extend_from_slice(&self.request_id.to_le_bytes());
        payload.extend_from_slice(&self.catalog_revision.to_le_bytes());
        payload.extend_from_slice(&self.width.to_le_bytes());
        payload.extend_from_slice(&self.height.to_le_bytes());
        payload.extend_from_slice(&self.target_fps.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload
    }
}

impl QualityResult {
    pub const BYTES: usize = 14;
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if payload.len() != Self::BYTES {
            return Err(invalid("quality result payload must be 14 bytes"));
        }
        Ok(Self {
            request_id: u32::from_le_bytes(payload[0..4].try_into().unwrap()),
            status: payload[4],
            reason: payload[5],
            applied_width: u16::from_le_bytes(payload[6..8].try_into().unwrap()),
            applied_height: u16::from_le_bytes(payload[8..10].try_into().unwrap()),
            applied_fps: u16::from_le_bytes(payload[10..12].try_into().unwrap()),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(Self::BYTES);
        payload.extend_from_slice(&self.request_id.to_le_bytes());
        payload.push(self.status);
        payload.push(self.reason);
        payload.extend_from_slice(&self.applied_width.to_le_bytes());
        payload.extend_from_slice(&self.applied_height.to_le_bytes());
        payload.extend_from_slice(&self.applied_fps.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload
    }
}

pub const CLIENT_AUTH_VERSION: u8 = 2;
pub const CLIENT_PUBLIC_KEY_BYTES: usize = 65;
pub const PHONE_BINDING_BYTES: usize = 32;
pub const AUTH_USB: u8 = 0;
pub const AUTH_LAN: u8 = 1;
pub const CLIENT_AUTH_MIN_BYTES: usize = 1 + CLIENT_PUBLIC_KEY_BYTES + 1 + SIGNATURE_BYTES;
pub const CLIENT_AUTH_MAX_BYTES: usize = CLIENT_AUTH_MIN_BYTES + MAX_CLIENT_LABEL;

fn valid_transport(transport: u8) -> bool {
    matches!(transport, AUTH_USB | AUTH_LAN)
}
fn valid_phone_binding(transport: u8, binding: &[u8]) -> bool {
    binding.len() == PHONE_BINDING_BYTES
        && if transport == AUTH_USB {
            binding.iter().all(|byte| *byte == 0)
        } else {
            binding.iter().any(|byte| *byte != 0)
        }
}
fn valid_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= MAX_CLIENT_LABEL
        && !label.chars().any(|c| {
            matches!(
                unicode_general_category::get_general_category(c),
                unicode_general_category::GeneralCategory::Control
                    | unicode_general_category::GeneralCategory::Format
                    | unicode_general_category::GeneralCategory::LineSeparator
                    | unicode_general_category::GeneralCategory::ParagraphSeparator
            )
        })
}

/// Domain-separated proof of the point and label over this phone session's
/// transport, phone certificate binding and one-use nonce. A nonce is not a
/// general-purpose signing request.
pub fn client_auth_message(
    transport: u8,
    phone_binding: &[u8],
    nonce: &[u8],
    public_key: &[u8],
    label: &str,
) -> io::Result<Vec<u8>> {
    if !valid_transport(transport)
        || !valid_phone_binding(transport, phone_binding)
        || nonce.len() != NONCE_BYTES
        || public_key.len() != CLIENT_PUBLIC_KEY_BYTES
        || public_key[0] != 4
        || !valid_label(label)
    {
        return Err(invalid("invalid authorization transcript fields"));
    }
    let mut bytes = b"AMB1 PC authorization\0v2\0".to_vec();
    bytes.push(transport);
    bytes.extend_from_slice(phone_binding);
    bytes.extend_from_slice(nonce);
    bytes.extend_from_slice(public_key);
    bytes.push(label.len() as u8);
    bytes.extend_from_slice(label.as_bytes());
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientChallenge {
    pub version: u8,
    pub transport: u8,
    pub phone_binding: Vec<u8>,
    pub nonce: Vec<u8>,
}
impl ClientChallenge {
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if payload.len() != 2 + PHONE_BINDING_BYTES + NONCE_BYTES {
            return Err(invalid("client challenge payload must be 66 bytes"));
        }
        if payload[0] != CLIENT_AUTH_VERSION || !valid_transport(payload[1]) {
            return Err(invalid("unsupported client challenge version or transport"));
        }
        if !valid_phone_binding(payload[1], &payload[2..2 + PHONE_BINDING_BYTES]) {
            return Err(invalid("invalid phone certificate binding"));
        }
        Ok(Self {
            version: payload[0],
            transport: payload[1],
            phone_binding: payload[2..2 + PHONE_BINDING_BYTES].to_vec(),
            nonce: payload[2 + PHONE_BINDING_BYTES..].to_vec(),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        assert_eq!(self.version, CLIENT_AUTH_VERSION);
        assert!(valid_transport(self.transport));
        assert!(valid_phone_binding(self.transport, &self.phone_binding));
        assert_eq!(self.nonce.len(), NONCE_BYTES);
        let mut payload = vec![self.version, self.transport];
        payload.extend_from_slice(&self.phone_binding);
        payload.extend_from_slice(&self.nonce);
        payload
    }
}

/// Unreleased v2 transfers the public point; the receiver derives its ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientAuth {
    pub version: u8,
    pub public_key: Vec<u8>,
    pub label: String,
    pub signature: Vec<u8>,
}
impl ClientAuth {
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if !(CLIENT_AUTH_MIN_BYTES..=CLIENT_AUTH_MAX_BYTES).contains(&payload.len()) {
            return Err(invalid("invalid client auth length"));
        }
        if payload[0] != CLIENT_AUTH_VERSION {
            return Err(invalid("unsupported client auth version"));
        }
        let offset = 1 + CLIENT_PUBLIC_KEY_BYTES;
        let label_length = payload[offset] as usize;
        if label_length > MAX_CLIENT_LABEL {
            return Err(invalid("client label exceeds the limit"));
        }
        if payload.len() != CLIENT_AUTH_MIN_BYTES + label_length {
            return Err(invalid("client auth length does not match its label"));
        }
        let label = std::str::from_utf8(&payload[offset + 1..offset + 1 + label_length])
            .map_err(|_| invalid("client label is not valid UTF-8"))?;
        if !valid_label(label) || payload[1] != 4 {
            return Err(invalid("invalid client point or label"));
        }
        Ok(Self {
            version: payload[0],
            public_key: payload[1..offset].to_vec(),
            label: label.to_owned(),
            signature: payload[payload.len() - SIGNATURE_BYTES..].to_vec(),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        assert_eq!(self.version, CLIENT_AUTH_VERSION);
        assert_eq!(self.public_key.len(), CLIENT_PUBLIC_KEY_BYTES);
        assert_eq!(self.public_key[0], 4);
        assert_eq!(self.signature.len(), SIGNATURE_BYTES);
        assert!(valid_label(&self.label));
        let mut payload = vec![self.version];
        payload.extend_from_slice(&self.public_key);
        payload.push(self.label.len() as u8);
        payload.extend_from_slice(self.label.as_bytes());
        payload.extend_from_slice(&self.signature);
        payload
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientAuthResult {
    pub status: u8,
    pub reason: u8,
    pub expires_at_unix_seconds: u32,
}
impl ClientAuthResult {
    pub const BYTES: usize = 6;
    fn validate(self) -> io::Result<()> {
        let valid = match self.status {
            AUTH_AUTHORIZED => self.reason == AUTH_REASON_NONE && self.expires_at_unix_seconds > 0,
            AUTH_PENDING => self.reason == AUTH_REASON_NONE && self.expires_at_unix_seconds == 0,
            AUTH_REJECTED => {
                (AUTH_REASON_UNKNOWN..=AUTH_REASON_BAD_SIGNATURE).contains(&self.reason)
                    && (self.reason == AUTH_REASON_EXPIRED || self.expires_at_unix_seconds == 0)
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(invalid("invalid client authorization outcome"))
        }
    }
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if payload.len() != Self::BYTES {
            return Err(invalid("client auth result payload must be 6 bytes"));
        }
        let result = Self {
            status: payload[0],
            reason: payload[1],
            expires_at_unix_seconds: u32::from_le_bytes(payload[2..6].try_into().unwrap()),
        };
        result.validate()?;
        Ok(result)
    }
    pub fn encode(&self) -> Vec<u8> {
        self.validate().expect("valid authorization outcome");
        let mut payload = vec![self.status, self.reason];
        payload.extend_from_slice(&self.expires_at_unix_seconds.to_le_bytes());
        payload
    }
}

impl Epoch {
    pub const BYTES: usize = 8;
    pub fn decode(payload: &[u8]) -> io::Result<Self> {
        if payload.len() != Self::BYTES {
            return Err(invalid("epoch payload must be 8 bytes"));
        }
        Ok(Self {
            request_id: u32::from_le_bytes(payload[0..4].try_into().unwrap()),
            stream_epoch: u32::from_le_bytes(payload[4..8].try_into().unwrap()),
        })
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(Self::BYTES);
        payload.extend_from_slice(&self.request_id.to_le_bytes());
        payload.extend_from_slice(&self.stream_epoch.to_le_bytes());
        payload
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub flags: u16,
    pub sequence: u32,
    pub payload_len: u32,
    pub pts_us: u64,
}

impl Header {
    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() != HEADER_SIZE || &bytes[..4] != b"AMB1" || bytes[4] != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid AMB1 header",
            ));
        }
        let header = Self {
            kind: bytes[5],
            flags: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            sequence: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            payload_len: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            pts_us: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        };
        if header.payload_len as usize > MAX_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "AMB1 payload exceeds 16 MiB",
            ));
        }
        header.validate_payload_size()?;
        Ok(header)
    }

    pub(crate) fn validate_payload_size(self) -> io::Result<()> {
        let length = self.payload_len as usize;
        let valid = match self.kind {
            CLIENT_CHALLENGE => length == 2 + PHONE_BINDING_BYTES + NONCE_BYTES,
            CLIENT_AUTH => (CLIENT_AUTH_MIN_BYTES..=CLIENT_AUTH_MAX_BYTES).contains(&length),
            CLIENT_AUTH_RESULT => length == ClientAuthResult::BYTES,
            // version + count + 4-byte addresses + SHA-256 + 128-bit key.
            PAIRING_INFO => PairingInfo::fits(length),
            QUALITY_CAPABILITIES => (1..=MAX_CONTROL_PAYLOAD).contains(&length),
            PREPARE_QUALITY | QUALITY_RESULT => length == 14,
            PAUSED | COMMIT_QUALITY | PING | PONG => length == 8,
            // A host hello is a signal, not a message: no payload to get wrong.
            HOST_HELLO => length == 0,
            VIDEO_CONFIG => length == 16,
            IDR_REQUEST | HELLO_ACK => length == 0,
            HELLO => length == 0 || length == 16,
            _ => true,
        };
        if !valid {
            return Err(invalid("invalid type-specific AMB1 payload length"));
        }
        Ok(())
    }

    pub fn encode(self) -> io::Result<[u8; HEADER_SIZE]> {
        if self.payload_len as usize > MAX_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AMB1 payload exceeds 16 MiB",
            ));
        }
        self.validate_payload_size()?;
        let mut bytes = [0; HEADER_SIZE];
        bytes[..4].copy_from_slice(b"AMB1");
        bytes[4] = 1;
        bytes[5] = self.kind;
        bytes[6..8].copy_from_slice(&self.flags.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.sequence.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.payload_len.to_le_bytes());
        bytes[16..24].copy_from_slice(&self.pts_us.to_le_bytes());
        Ok(bytes)
    }
}

#[derive(Debug)]
pub struct Frame {
    pub header: Header,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: u8, sequence: u32, payload: Vec<u8>) -> io::Result<Self> {
        let payload_len = u32::try_from(payload.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "payload overflow"))?;
        let header = Header {
            kind,
            flags: 0,
            sequence,
            payload_len,
            pts_us: 0,
        };
        header.encode()?;
        Ok(Self { header, payload })
    }
    pub fn validate(&self) -> io::Result<()> {
        self.header.encode()?;
        if self.payload.len() != self.header.payload_len as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "payload length mismatch",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pairing_info_round_trips_and_rejects_malformed_payloads() {
        let mut payload = vec![PairingInfo::VERSION, 2];
        payload.extend_from_slice(&[192, 168, 1, 3]);
        payload.extend_from_slice(&[100, 66, 21, 23]);
        payload.extend_from_slice(&[0xab; 32]);
        payload.extend_from_slice(&[0xcd; 16]);
        let info = PairingInfo::decode(&payload).unwrap();
        assert_eq!(
            info.addresses,
            vec![
                std::net::Ipv4Addr::new(192, 168, 1, 3),
                std::net::Ipv4Addr::new(100, 66, 21, 23)
            ]
        );
        assert_eq!(info.fingerprint, "ab".repeat(32));
        assert_eq!(info.key, "cd".repeat(16));
        // The private address wins over the tunnel address even when listed second.
        assert_eq!(
            info.preferred_address(),
            Some(std::net::Ipv4Addr::new(192, 168, 1, 3))
        );

        // Wrong version, truncated, trailing bytes and a zero count are all refused.
        let mut wrong_version = payload.clone();
        wrong_version[0] = 2;
        assert!(PairingInfo::decode(&wrong_version).is_err());
        assert!(PairingInfo::decode(&payload[..payload.len() - 1]).is_err());
        let mut trailing = payload.clone();
        trailing.push(0);
        assert!(PairingInfo::decode(&trailing).is_err());
        let mut no_addresses = vec![PairingInfo::VERSION, 0];
        no_addresses.extend_from_slice(&[0xab; 32]);
        no_addresses.extend_from_slice(&[0xcd; 16]);
        assert!(PairingInfo::decode(&no_addresses).is_err());
    }

    #[test]
    fn the_length_check_matches_the_decoder() {
        let good = 2 + 4 + PairingInfo::FINGERPRINT_BYTES + PairingInfo::KEY_BYTES;
        assert!(PairingInfo::fits(good));
        assert!(!PairingInfo::fits(good - 1));
        assert!(!PairingInfo::fits(good + 1));
        let four = 2 + 4 * 4 + PairingInfo::FINGERPRINT_BYTES + PairingInfo::KEY_BYTES;
        assert!(PairingInfo::fits(four));
        assert!(!PairingInfo::fits(
            2 + 5 * 4 + PairingInfo::FINGERPRINT_BYTES + PairingInfo::KEY_BYTES
        ));
    }

    use super::*;
    #[test]
    fn frozen_cpp_header_and_unsigned_pts() {
        let fixture = [
            65, 77, 66, 49, 1, 17, 52, 18, 68, 51, 34, 17, 8, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1,
        ];
        let header = Header::decode(&fixture).unwrap();
        assert_eq!(header.flags, 0x1234);
        assert_eq!(header.sequence, 0x11223344);
        assert_eq!(header.pts_us, 0x0102030405060708);
        assert_eq!(header.encode().unwrap(), fixture);
        let header = Header {
            pts_us: 0x8877665544332211,
            kind: 0xfe,
            ..header
        };
        assert_eq!(Header::decode(&header.encode().unwrap()).unwrap(), header);
    }
    #[test]
    fn rejects_corruption_before_allocation() {
        let mut bytes = Header {
            kind: VIDEO_AU,
            flags: 0,
            sequence: 1,
            payload_len: 0,
            pts_us: 0,
        }
        .encode()
        .unwrap();
        bytes[12..16].copy_from_slice(&((MAX_PAYLOAD + 1) as u32).to_le_bytes());
        assert!(Header::decode(&bytes).is_err());
        bytes[0] = 0;
        assert!(Header::decode(&bytes).is_err());
        assert!(Header::decode(&bytes[..23]).is_err());
    }

    fn fixture_capabilities() -> Capabilities {
        Capabilities {
            extension_version: 1,
            permission: PERMISSION_READY,
            catalog_revision: 7,
            stream_epoch: 42,
            modes: vec![
                QualityMode {
                    width: 1280,
                    height: 720,
                    target_fps: 30,
                    bitrate: 6_000_000,
                },
                QualityMode {
                    width: 1920,
                    height: 1080,
                    target_fps: 30,
                    bitrate: 13_500_000,
                },
                QualityMode {
                    width: 1280,
                    height: 720,
                    target_fps: 60,
                    bitrate: 12_000_000,
                },
            ],
        }
    }

    /// The canonical golden fixture both languages read. Pins the type
    /// number, the field order and the counted list layout.
    /// The per-PC authorization frames are canonical: both languages decode this
    /// fixture, assert every field, and re-encode to the identical bytes, so the
    /// type numbers, field order and counted label cannot drift.
    #[test]
    fn client_auth_frames_match_the_golden_fixture() {
        let fixture: &[u8] = include_bytes!("../../protocol/fixtures/client-auth-v2.bin");

        let mut offset = 0;
        let challenge_len = HEADER_SIZE + 66;
        let challenge_header = Header::decode(&fixture[offset..offset + HEADER_SIZE]).unwrap();
        assert_eq!(challenge_header.kind, CLIENT_CHALLENGE);
        let challenge =
            ClientChallenge::decode(&fixture[offset + HEADER_SIZE..offset + challenge_len])
                .unwrap();
        assert_eq!(challenge.version, CLIENT_AUTH_VERSION);
        assert_eq!(challenge.transport, AUTH_LAN);
        assert_eq!(challenge.phone_binding, (32..64).collect::<Vec<_>>());
        assert_eq!(challenge.nonce, (0..32).collect::<Vec<_>>());
        let mut encoded = challenge_header.encode().unwrap().to_vec();
        encoded.extend_from_slice(&challenge.encode());
        assert_eq!(
            encoded,
            &fixture[offset..offset + challenge_len],
            "client challenge"
        );
        offset += challenge_len;

        let auth_len = HEADER_SIZE + CLIENT_AUTH_MIN_BYTES + "studio-pc".len();
        let auth_header = Header::decode(&fixture[offset..offset + HEADER_SIZE]).unwrap();
        assert_eq!(auth_header.kind, CLIENT_AUTH);
        let auth = ClientAuth::decode(&fixture[offset + HEADER_SIZE..offset + auth_len]).unwrap();
        assert_eq!(auth.version, CLIENT_AUTH_VERSION);
        assert_eq!(
            auth.public_key,
            include_bytes!("../../protocol/fixtures/client-signature-v1.bin")[..65]
        );
        assert_eq!(auth.label, "studio-pc");
        assert_eq!(auth.signature.len(), SIGNATURE_BYTES);
        let mut encoded = auth_header.encode().unwrap().to_vec();
        encoded.extend_from_slice(&auth.encode());
        assert_eq!(encoded, &fixture[offset..offset + auth_len], "client auth");
        offset += auth_len;

        let result_len = HEADER_SIZE + 6;
        let result_header = Header::decode(&fixture[offset..offset + HEADER_SIZE]).unwrap();
        assert_eq!(result_header.kind, CLIENT_AUTH_RESULT);
        let result =
            ClientAuthResult::decode(&fixture[offset + HEADER_SIZE..offset + result_len]).unwrap();
        assert_eq!(
            (result.status, result.reason),
            (AUTH_AUTHORIZED, AUTH_REASON_NONE)
        );
        assert_eq!(result.expires_at_unix_seconds, 1_800_000_000);
        let mut encoded = result_header.encode().unwrap().to_vec();
        encoded.extend_from_slice(&result.encode());
        assert_eq!(
            encoded,
            &fixture[offset..offset + result_len],
            "client auth result"
        );
        offset += result_len;

        assert_eq!(offset, fixture.len(), "the fixture has no trailing bytes");
    }

    /// A counted label is validated before anything is allocated, and a length
    /// that disagrees with the payload is refused rather than truncated.
    #[test]
    fn client_auth_label_length_is_validated_before_allocation() {
        let mut payload = ClientAuth {
            version: CLIENT_AUTH_VERSION,
            public_key: include_bytes!("../../protocol/fixtures/client-signature-v1.bin")[..65]
                .to_vec(),
            label: "ok".into(),
            signature: vec![0; SIGNATURE_BYTES],
        }
        .encode();
        // Claim a label far longer than the payload carries.
        payload[1 + CLIENT_PUBLIC_KEY_BYTES] = 200;
        let error = ClientAuth::decode(&payload).unwrap_err();
        assert!(error.to_string().contains("exceeds the limit"), "{error}");

        // A length that fits the ceiling but not the payload is refused too.
        let mut short = payload.clone();
        short[1 + CLIENT_PUBLIC_KEY_BYTES] = MAX_CLIENT_LABEL as u8;
        let error = ClientAuth::decode(&short).unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
    }

    #[test]
    fn the_domain_separated_transcript_matches_the_shared_bytes() {
        let key = &include_bytes!("../../protocol/fixtures/client-signature-v1.bin")[..65];
        let message = client_auth_message(
            AUTH_LAN,
            &(32..64).collect::<Vec<_>>(),
            &(0..32).collect::<Vec<_>>(),
            key,
            "studio-pc",
        )
        .unwrap();
        assert_eq!(
            message,
            include_bytes!("../../protocol/fixtures/client-auth-transcript-v2.bin")
        );
        assert_eq!(
            client_auth_message(
                AUTH_USB,
                &[0; 32],
                &(0..32).collect::<Vec<_>>(),
                key,
                "studio-pc"
            )
            .unwrap(),
            include_bytes!("../../protocol/fixtures/client-auth-transcript-usb-v2.bin")
        );
    }

    #[test]
    fn auth_utf8_and_outcome_combinations_are_strict() {
        let key = &include_bytes!("../../protocol/fixtures/client-signature-v1.bin")[..65];
        let mut payload = ClientAuth {
            version: CLIENT_AUTH_VERSION,
            public_key: key.to_vec(),
            label: "ok".into(),
            signature: vec![0; 64],
        }
        .encode();
        payload[67] = 0xc0;
        payload[68] = 0xaf;
        assert!(ClientAuth::decode(&payload).is_err());
        for result in [
            [3, 0, 0, 0, 0, 0],
            [0, 5, 1, 0, 0, 0],
            [1, 0, 1, 0, 0, 0],
            [2, 0, 0, 0, 0, 0],
            [0, 0, 0, 0, 0, 0],
        ] {
            assert!(ClientAuthResult::decode(&result).is_err());
        }
    }

    #[test]
    fn auth_length_bounds_fail_from_the_header_before_allocation() {
        let frame = Frame::new(PING, 0, vec![0; 8]).unwrap();
        for (kind, length) in [
            (CLIENT_CHALLENGE, 33u32),
            (CLIENT_CHALLENGE, 35),
            (CLIENT_AUTH, 91),
            (CLIENT_AUTH, (CLIENT_AUTH_MAX_BYTES + 1) as u32),
            (CLIENT_AUTH_RESULT, 7),
            (CLIENT_AUTH_RESULT, 1_000_000),
        ] {
            let mut header = frame.header.encode().unwrap();
            header[5] = kind;
            header[12..16].copy_from_slice(&length.to_le_bytes());
            assert!(Header::decode(&header).is_err());
        }
    }

    #[test]
    fn quality_capabilities_match_the_golden_fixture() {
        let fixture = include_bytes!("../../protocol/fixtures/quality-capabilities-v1.bin");
        let header = Header::decode(&fixture[..HEADER_SIZE]).unwrap();
        assert_eq!(header.kind, QUALITY_CAPABILITIES);
        let capabilities = Capabilities::decode(&fixture[HEADER_SIZE..]).unwrap();
        assert_eq!(capabilities, fixture_capabilities());
        // Re-encoding the decoded values reproduces the fixture bytes.
        let mut frame = header.encode().unwrap().to_vec();
        frame.extend_from_slice(&capabilities.encode().unwrap());
        assert_eq!(frame, fixture);
    }

    #[test]
    fn auth_labels_reject_format_controls_including_supplementary_tags() {
        assert!(valid_label("camera-猫😺"));
        for label in [
            "pc\n",
            "pc\u{202e}",
            "pc\u{e0001}",
            "pc\u{e0020}",
            "pc\u{2028}",
            "pc\u{2029}",
        ] {
            assert!(!valid_label(label), "unsafe label {label:?}");
        }
    }

    #[test]
    fn quality_payload_length_is_validated_before_allocation() {
        // A count that does not match the payload is rejected before the
        // mode list is trusted or built.
        let mut payload = fixture_capabilities().encode().unwrap();
        payload.truncate(payload.len() - QUALITY_MODE_BYTES);
        assert!(Capabilities::decode(&payload).is_err());
        // An over-limit count is rejected, never silently truncated.
        payload[8..10].copy_from_slice(&(MAX_QUALITY_MODES as u16 + 1).to_le_bytes());
        assert!(Capabilities::decode(&payload).is_err());
        // Truncated fixed fields.
        assert!(Capabilities::decode(&payload[..4]).is_err());
    }

    #[test]
    fn maximum_mode_list_round_trips_and_over_limit_is_refused() {
        let modes: Vec<QualityMode> = (0..MAX_QUALITY_MODES)
            .map(|index| QualityMode {
                width: 640 + (index % 32) as u16 * 2,
                height: 480 + (index % 16) as u16 * 2,
                target_fps: 30,
                bitrate: 1_000_000,
            })
            .collect();
        let capabilities = Capabilities {
            modes,
            ..fixture_capabilities()
        };
        let payload = capabilities.encode().unwrap();
        assert_eq!(Capabilities::decode(&payload).unwrap(), capabilities);
        // One mode over the limit is refused by the encoder, never truncated.
        let mut over = fixture_capabilities();
        over.modes = vec![over.modes[0]; MAX_QUALITY_MODES + 1];
        assert!(over.encode().is_err());
    }

    #[test]
    fn quality_control_frames_round_trip() {
        let prepare = PrepareQuality {
            request_id: 9,
            catalog_revision: 7,
            width: 1920,
            height: 1080,
            target_fps: 30,
        };
        let payload = prepare.encode();
        assert_eq!(payload.len(), PrepareQuality::BYTES);
        assert_eq!(PrepareQuality::decode(&payload).unwrap(), prepare);

        let result = QualityResult {
            request_id: 9,
            status: RESULT_APPLIED,
            reason: REASON_NONE,
            applied_width: 1920,
            applied_height: 1080,
            applied_fps: 30,
        };
        let payload = result.encode();
        assert_eq!(QualityResult::decode(&payload).unwrap(), result);

        for value in [
            Epoch {
                request_id: 9,
                stream_epoch: 43,
            },
            Epoch {
                request_id: 9,
                stream_epoch: 42,
            },
        ] {
            assert_eq!(Epoch::decode(&value.encode()).unwrap(), value);
        }
        // Paused and CommitQuality share the epoch frame shape.
        assert_eq!(Epoch::BYTES, 8);
        assert!(Epoch::decode(&[0; 7]).is_err());
    }
}

/// The Wi-Fi pairing details a phone sends over the USB cable.
///
/// The payload is deliberately tiny and self-describing: a version, up to four
/// IPv4 addresses (a phone can be multi-homed), the SHA-256 of the phone's TLS
/// certificate and the 128-bit access key. It carries exactly what the PC's three
/// pairing fields want, so receiving it is equivalent to the user typing them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingInfo {
    pub addresses: Vec<std::net::Ipv4Addr>,
    pub fingerprint: String,
    pub key: String,
}

impl PairingInfo {
    pub const VERSION: u8 = 1;
    pub const MAX_ADDRESSES: usize = 4;
    pub const KEY_BYTES: usize = 16;
    pub const FINGERPRINT_BYTES: usize = 32;

    fn fits(length: usize) -> bool {
        // 1 version + 1 count + addresses + fingerprint + key
        if length < 2 + 4 + Self::FINGERPRINT_BYTES + Self::KEY_BYTES {
            return false;
        }
        let count = length.saturating_sub(2 + Self::FINGERPRINT_BYTES + Self::KEY_BYTES) / 4;
        (1..=Self::MAX_ADDRESSES).contains(&count)
            && length == 2 + count * 4 + Self::FINGERPRINT_BYTES + Self::KEY_BYTES
    }

    pub fn decode(payload: &[u8]) -> crate::Result<Self> {
        if payload.len() < 2 {
            return Err("pairing info is too short".into());
        }
        if payload[0] != Self::VERSION {
            return Err("unexpected pairing info version".into());
        }
        let count = payload[1] as usize;
        if !(1..=Self::MAX_ADDRESSES).contains(&count) {
            return Err("pairing info carries an unusable address count".into());
        }
        let expected = 2 + count * 4 + Self::FINGERPRINT_BYTES + Self::KEY_BYTES;
        if payload.len() != expected {
            return Err("pairing info has trailing or missing bytes".into());
        }
        let mut addresses = Vec::with_capacity(count);
        for index in 0..count {
            let at = 2 + index * 4;
            addresses.push(std::net::Ipv4Addr::new(
                payload[at],
                payload[at + 1],
                payload[at + 2],
                payload[at + 3],
            ));
        }
        let fingerprint_at = 2 + count * 4;
        let key_at = fingerprint_at + Self::FINGERPRINT_BYTES;
        Ok(Self {
            addresses,
            fingerprint: hex(&payload[fingerprint_at..key_at]),
            key: hex(&payload[key_at..]),
        })
    }

    /// The address to try first: a private LAN address beats a tunnel or a public
    /// one, because the phone and the PC are on the same network in the ordinary
    /// case and the other addresses are usually VPNs.
    pub fn preferred_address(&self) -> Option<std::net::Ipv4Addr> {
        let private =
            |address: &std::net::Ipv4Addr| address.is_private() || address.is_link_local();
        self.addresses
            .iter()
            .find(|address| private(address))
            .or_else(|| self.addresses.first())
            .copied()
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}
