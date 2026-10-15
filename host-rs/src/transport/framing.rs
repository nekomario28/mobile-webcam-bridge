use crate::wire::{self, Frame, Header};
use std::{io, time::Duration};

/// One partially received frame's lifetime. A frame that cannot complete inside
/// it terminates the connection instead of trickling indefinitely: a peer that
/// sends a byte at a time must not be able to extend its own deadline by
/// keeping the connection alive.
pub(crate) const FRAME_LIFETIME: Duration = Duration::from_secs(5);

/// AMB1's shape for the shared reader: a fixed-size header, then a payload
/// whose length is validated against the header's type *before* anything is
/// allocated or read. An over-limit count is refused, never truncated and
/// never honoured.
pub(crate) struct Amb1;

impl io_budget::Framed for Amb1 {
    type Header = Header;
    type Frame = Frame;

    fn header_len() -> usize {
        wire::HEADER_SIZE
    }

    fn decode_header(bytes: &[u8]) -> io::Result<Header> {
        Header::decode(bytes)
    }

    fn payload_len(header: &Header) -> io::Result<usize> {
        header.validate_payload_size()?;
        Ok(header.payload_len as usize)
    }

    fn assemble(header: Header, payload: Vec<u8>) -> Frame {
        Frame { header, payload }
    }
}

/// The reader both byte-stream TCP and split-transfer USB own. Partial
/// header/payload progress survives a poll timeout, and one frame has one
/// lifetime; both rules and their regressions live in the `io-budget` crate.
pub(crate) struct FrameReader(io_budget::FrameReader<Amb1>);

impl Default for FrameReader {
    fn default() -> Self {
        Self(io_budget::FrameReader::new(FRAME_LIFETIME))
    }
}

impl std::ops::Deref for FrameReader {
    type Target = io_budget::FrameReader<Amb1>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for FrameReader {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AMB1-specific: the counted length is checked against the header's type
    /// before a single payload byte is read or allocated. The generic
    /// split-poll and trickle-lifetime rules are covered by the `io-budget`
    /// crate's own regressions.
    #[test]
    fn control_header_is_rejected_before_payload_read_or_allocation() {
        let frame = Frame::new(wire::QUALITY_RESULT, 1, vec![0; 14]).unwrap();
        let mut header = frame.header.encode().unwrap();
        header[12..16].copy_from_slice(&1_000_000u32.to_le_bytes());
        let mut sent = false;
        let error = FrameReader::default()
            .poll(Duration::from_millis(10), |target, _| {
                assert!(!sent, "invalid control length reached payload I/O");
                target.copy_from_slice(&header);
                sent = true;
                Ok(header.len())
            })
            .unwrap_err();
        assert!(error.to_string().contains("type-specific"));
    }
}
