use crate::{settings::Settings, usb::DeviceId};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};

pub const MAX_LINE: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Start {
        settings: Settings,
        device: Option<DeviceId>,
    },
    Transform {
        rotation: u16,
        mirror: bool,
        vertical_flip: bool,
    },
    Stop {},
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    UsbPermission {
        device: DeviceId,
    },
    Connecting {},
    Waiting {},
    Stats {
        decoded: u64,
        submitted: u64,
        active: bool,
        width: u32,
        height: u32,
        decoder: String,
    },
    Error {
        message: String,
    },
    Stopped {},
}

pub fn read<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> crate::Result<Option<T>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("truncated control message".into())
            };
        }
        let count = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |n| n + 1);
        if bytes.len() + count > MAX_LINE {
            return Err("control message exceeds 16 KiB".into());
        }
        let finished = chunk[count - 1] == b'\n';
        bytes.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if finished {
            return Ok(Some(serde_json::from_slice(&bytes)?));
        }
    }
}

pub fn write<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    if bytes.len() >= MAX_LINE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "control message too large",
        ));
    }
    writer.write_all(&bytes)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_messages_and_unknown_fields() {
        assert!(read::<Command>(&mut io::Cursor::new(vec![b'x'; MAX_LINE + 1])).is_err());
        assert!(
            read::<Command>(&mut io::Cursor::new(
                b"{\"command\":\"stop\",\"extra\":1}\n"
            ))
            .is_err()
        );
        assert!(read::<Command>(&mut io::Cursor::new(b"{\"command\":\"stop\"}")).is_err());
        assert!(matches!(
            read::<Command>(&mut io::Cursor::new(b"{\"command\":\"stop\"}\n")).unwrap(),
            Some(Command::Stop {})
        ));
    }
}
