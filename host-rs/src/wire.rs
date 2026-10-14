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
pub const VIDEO_CONFIG: u8 = 0x10;
pub const VIDEO_AU: u8 = 0x11;
pub const IDR_REQUEST: u8 = 0x12;
pub const KEYFRAME: u16 = 1;
pub const CONFIG_INCLUDED: u16 = 2;
pub const DISCONTINUITY: u16 = 4;

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
pub const REASON_NONE: u8 = 0;
pub const REASON_PERMISSION: u8 = 1;
pub const REASON_FOREGROUND: u8 = 2;
pub const REASON_UNSUPPORTED_MODE: u8 = 3;
pub const REASON_STALE_REVISION: u8 = 4;
pub const REASON_BUSY: u8 = 5;
pub const REASON_DEADLINE: u8 = 6;

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

    fn validate_payload_size(self) -> io::Result<()> {
        let length = self.payload_len as usize;
        let valid = match self.kind {
            QUALITY_CAPABILITIES => (1..=MAX_CONTROL_PAYLOAD).contains(&length),
            PREPARE_QUALITY | QUALITY_RESULT => length == 14,
            PAUSED | COMMIT_QUALITY | PING | PONG => length == 8,
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
