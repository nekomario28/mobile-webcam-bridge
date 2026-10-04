use std::io;

pub const HEADER_SIZE: usize = 24;
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;
pub const HELLO: u8 = 0x01;
pub const HELLO_ACK: u8 = 0x02;
pub const PING: u8 = 0x03;
pub const PONG: u8 = 0x04;
pub const VIDEO_CONFIG: u8 = 0x10;
pub const VIDEO_AU: u8 = 0x11;
pub const IDR_REQUEST: u8 = 0x12;
pub const KEYFRAME: u16 = 1;
pub const CONFIG_INCLUDED: u16 = 2;
pub const DISCONTINUITY: u16 = 4;

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
        Ok(header)
    }

    pub fn encode(self) -> io::Result<[u8; HEADER_SIZE]> {
        if self.payload_len as usize > MAX_PAYLOAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "AMB1 payload exceeds 16 MiB",
            ));
        }
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
}
