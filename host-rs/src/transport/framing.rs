use crate::wire::{self, Frame, Header};
use std::{
    io,
    time::{Duration, Instant},
};

/// A bounded partial frame survives polls, but not an indefinitely trickled
/// frame. Both byte-stream TCP and split-transfer USB use this same owner.
pub(crate) struct FrameReader {
    header_bytes: [u8; wire::HEADER_SIZE],
    header_offset: usize,
    header: Option<Header>,
    payload: Vec<u8>,
    payload_offset: usize,
    frame_started: Option<Instant>,
}
impl Default for FrameReader {
    fn default() -> Self {
        Self {
            header_bytes: [0; wire::HEADER_SIZE],
            header_offset: 0,
            header: None,
            payload: Vec::new(),
            payload_offset: 0,
            frame_started: None,
        }
    }
}
impl FrameReader {
    pub fn poll(
        &mut self,
        timeout: Duration,
        mut read: impl FnMut(&mut [u8], Duration) -> crate::Result<usize>,
    ) -> crate::Result<Option<Frame>> {
        let poll_deadline = Instant::now() + timeout;
        loop {
            let now = Instant::now();
            let frame_deadline = self
                .frame_started
                .map(|start| start + Duration::from_secs(5));
            if frame_deadline.is_some_and(|deadline| now >= deadline) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete AMB1 frame deadline expired",
                )
                .into());
            }
            if self.header_offset == wire::HEADER_SIZE && self.header.is_none() {
                let header = Header::decode(&self.header_bytes)?;
                self.payload = vec![0; header.payload_len as usize];
                self.header = Some(header);
            }
            if let Some(header) = self.header
                && self.payload_offset == self.payload.len()
            {
                let payload = std::mem::take(&mut self.payload);
                self.header_offset = 0;
                self.payload_offset = 0;
                self.header = None;
                self.frame_started = None;
                return Ok(Some(Frame { header, payload }));
            }
            let deadline =
                frame_deadline.map_or(poll_deadline, |deadline| deadline.min(poll_deadline));
            let Some(left) = deadline
                .checked_duration_since(now)
                .filter(|left| !left.is_zero())
            else {
                return Ok(None);
            };
            let target = if self.header.is_none() {
                &mut self.header_bytes[self.header_offset..]
            } else {
                &mut self.payload[self.payload_offset..]
            };
            let capacity = target.len();
            let count = match read(target, left.max(Duration::from_millis(1))) {
                Ok(0) => {
                    return Err(
                        io::Error::new(io::ErrorKind::UnexpectedEof, "peer disconnected").into(),
                    );
                }
                Ok(count) => count,
                Err(error)
                    if error.downcast_ref::<io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        )
                    }) =>
                {
                    return Ok(None);
                }
                Err(error)
                    if error
                        .downcast_ref::<io::Error>()
                        .is_some_and(|error| error.kind() == io::ErrorKind::Interrupted) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if count > capacity {
                return Err("transport returned an invalid byte count".into());
            }
            self.frame_started.get_or_insert(now);
            if self.header.is_none() {
                self.header_offset += count;
            } else {
                self.payload_offset += count;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split_usb_style_transfers_and_empty_polls_keep_one_frame() {
        let frame = Frame::new(wire::PONG, 42, vec![9; 8]).unwrap();
        let bytes = [frame.header.encode().unwrap().as_slice(), &frame.payload].concat();
        let mut reader = FrameReader::default();
        let mut offset = 0;
        let mut pauses = 0;
        let result = loop {
            let mut progressed = false;
            let result = reader
                .poll(Duration::from_millis(10), |buffer, _| {
                    if progressed {
                        return Err(io::Error::from(io::ErrorKind::TimedOut).into());
                    }
                    let count = buffer.len().min(3).min(bytes.len() - offset);
                    buffer[..count].copy_from_slice(&bytes[offset..offset + count]);
                    offset += count;
                    progressed = true;
                    Ok(count)
                })
                .unwrap();
            if let Some(frame) = result {
                break frame;
            }
            pauses += 1;
            assert!(pauses < 20);
        };
        assert!(pauses > 0);
        assert_eq!(result.header.sequence, 42);
        assert_eq!(result.payload, vec![9; 8]);
    }
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
    #[test]
    fn trickling_cannot_renew_the_partial_frame_deadline() {
        let mut reader = FrameReader {
            frame_started: Some(Instant::now() - Duration::from_secs(6)),
            ..Default::default()
        };
        let error = reader
            .poll(Duration::from_millis(10), |_, _| {
                panic!("expired partial frame must not read")
            })
            .unwrap_err();
        assert!(error.to_string().contains("frame deadline expired"));
    }
}
