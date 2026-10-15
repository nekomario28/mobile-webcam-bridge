//! AMB1 transports. Every connection speaks the same frame contract; the
//! socket framing, deadline handling and pairing exchange are owned here once
//! and shared by every `TcpStream`-backed transport.
mod framing;
use crate::wire::{self, Frame, Header};
pub(crate) use framing::FrameReader;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

/// A bidirectional AMB1 conversation.
pub trait Transport {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame>;
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()>;
    /// A poll timeout is not a frame boundary: a partially received frame
    /// survives it and keeps its own lifetime. Required rather than defaulted,
    /// because a receive-based default consumes AMB1 bytes and then reports a
    /// timeout, losing the frame boundary.
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>>;
}

/// A byte stream that is both readable and writable; every socket transport
/// provides one.
pub trait ByteStream: Read + Write {}
impl<S: Read + Write> ByteStream for S {}

/// A `Transport` whose bytes flow through a `TcpStream`, optionally beneath a
/// security layer. Owns the shared framing, deadline handling and pairing
/// exchange; a socket transport only names its socket and its byte stream.
pub trait SocketTransport {
    /// The raw socket; read/write deadlines are enforced on it.
    fn socket(&mut self) -> &mut TcpStream;
    /// The byte stream AMB1 sees: raw TCP, or TLS over the same socket.
    fn stream(&mut self) -> &mut dyn ByteStream;

    /// Install one operation deadline, including security-layer internal I/O.
    /// TLS transports override this to propagate it beneath record buffering.
    fn set_deadline(&mut self, deadline: Instant) -> io::Result<()> {
        let left = remaining(deadline)?;
        self.socket().set_read_timeout(Some(left))?;
        self.socket().set_write_timeout(Some(left))
    }

    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        self.receive_until(Instant::now() + timeout)
    }

    fn receive_until(&mut self, deadline: Instant) -> crate::Result<Frame> {
        let mut header = [0; wire::HEADER_SIZE];
        self.read_exact_until(&mut header, deadline)?;
        let header = Header::decode(&header)?;
        let mut payload = vec![0; header.payload_len as usize];
        self.read_exact_until(&mut payload, deadline)?;
        Ok(Frame { header, payload })
    }

    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        self.send_until(frame, Instant::now() + timeout)
    }

    fn send_until(&mut self, frame: &Frame, deadline: Instant) -> crate::Result<()> {
        frame.validate()?;
        let header = frame.header.encode()?;
        for bytes in [header.as_slice(), frame.payload.as_slice()] {
            let mut offset = 0;
            while offset < bytes.len() {
                self.set_deadline(deadline)?;
                match self.stream().write(&bytes[offset..]) {
                    Ok(0) => return Err("transport made no write progress".into()),
                    Ok(count) => offset += count,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        self.set_deadline(deadline)?;
        self.stream().flush()?;
        remaining(deadline)?;
        Ok(())
    }

    /// Read exactly `bytes.len()` bytes within the caller's operation deadline.
    /// A closed peer is an error, not a short read. Timeout is terminal: callers
    /// must close, not retry a partially consumed AMB1 frame.
    fn read_exact_until(&mut self, bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            self.set_deadline(deadline)?;
            match self.stream().read(&mut bytes[offset..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "peer disconnected",
                    ));
                }
                Ok(count) => offset += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        remaining(deadline)?;
        Ok(())
    }

    /// AMB1 pairing: HELLO carrying an optional credential, completed only by
    /// an empty HELLO_ACK with sequence 1. A server that closes the connection
    /// instead of answering has rejected the credential — the same signal the
    /// real phone produces for a wrong access key.
    fn pair_until(&mut self, credential: Vec<u8>, deadline: Instant) -> crate::Result<()> {
        self.send_until(&Frame::new(wire::HELLO, 1, credential)?, deadline)?;
        let ack = self.receive_until(deadline).map_err(|error| {
            match error.downcast_ref::<io::Error>() {
                Some(closed) if closed.kind() == io::ErrorKind::UnexpectedEof => {
                    "Wi-Fi pairing was not authorized".into()
                }
                _ => error,
            }
        })?;
        if ack.header.kind != wire::HELLO_ACK || ack.header.sequence != 1 || !ack.payload.is_empty()
        {
            return Err("Wi-Fi pairing was not authorized".into());
        }
        Ok(())
    }
    /// Poll for one complete AMB1 frame without losing a partially received
    /// one. Each read attempt installs the remaining budget through
    /// `set_deadline`, so a TLS transport enforces it below rustls' record
    /// buffering instead of only around the outer call. `Ok(None)` means the
    /// poll elapsed; the partial frame and its own lifetime are retained.
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>>;
}

impl<T: SocketTransport> Transport for T {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        SocketTransport::receive(self, timeout)
    }
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        SocketTransport::send(self, frame, timeout)
    }
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        SocketTransport::poll_receive(self, timeout)
    }
}

/// Time left on a deadline, never zero: some USB stacks read a zero timeout
/// as "wait forever". The rule and its regressions live in the `io-budget`
/// crate; this only converts the caller's absolute `Instant` deadline into a
/// budget measured from now.
pub(crate) fn remaining(deadline: Instant) -> io::Result<Duration> {
    let clock = io_budget::SystemClock;
    let budget =
        io_budget::Budget::for_duration(deadline.saturating_duration_since(Instant::now()), &clock);
    budget.remaining(io_budget::Clock::now(&clock))
}

/// Resolve a host to candidate addresses; a literal address is used as-is.
pub(crate) fn resolve(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    let host = host.trim();
    if host.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "enter an IP address",
        ));
    }
    match host.parse::<SocketAddr>() {
        Ok(address) => Ok(vec![address]),
        Err(_) => Ok((host, port).to_socket_addrs()?.collect()),
    }
}

/// Try every resolved address within one shared deadline. `attempt` receives
/// the connected socket and that deadline; the first attempt that connects
/// and pairs wins, otherwise the last error is reported.
pub(crate) fn dial<T>(
    host: &str,
    port: u16,
    timeout: Duration,
    mut attempt: impl FnMut(TcpStream, Instant) -> crate::Result<T>,
) -> crate::Result<T> {
    let deadline = Instant::now() + timeout;
    let mut last: Box<dyn std::error::Error + Send + Sync> = "no addresses".into();
    for address in resolve(host, port)? {
        let socket = match TcpStream::connect_timeout(&address, remaining(deadline)?) {
            Ok(socket) => socket,
            Err(error) => {
                last = Box::new(error);
                continue;
            }
        };
        match attempt(socket, deadline) {
            Ok(connected) => return Ok(connected),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// Plaintext AMB1 over TCP. Production Wi-Fi always uses the TLS transport;
/// this remains the framing test surface without certificate fixtures.
pub struct Tcp(pub TcpStream, FrameReader);

impl SocketTransport for Tcp {
    fn socket(&mut self) -> &mut TcpStream {
        &mut self.0
    }
    fn stream(&mut self) -> &mut dyn ByteStream {
        &mut self.0
    }
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        let socket = &mut self.0;
        self.1.poll(timeout, |bytes, left| {
            socket.set_read_timeout(Some(left))?;
            Ok(socket.read(bytes)?)
        })
    }
}

impl Tcp {
    pub fn new(socket: TcpStream) -> Self {
        Self(socket, FrameReader::default())
    }
    pub fn connect(host: &str, port: u16, timeout: Duration) -> crate::Result<Self> {
        dial(host, port, timeout, |socket, deadline| {
            socket.set_nodelay(true)?;
            let mut tcp = Self::new(socket);
            tcp.pair_until(Vec::new(), deadline)?;
            Ok(tcp)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, thread};
    #[test]
    fn copied_phone_endpoint_connects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let peer = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut tcp = Tcp::new(socket);
            let hello = Transport::receive(&mut tcp, Duration::from_secs(2)).unwrap();
            assert_eq!(hello.header.kind, wire::HELLO);
            Transport::send(
                &mut tcp,
                &Frame::new(wire::HELLO_ACK, hello.header.sequence, Vec::new()).unwrap(),
                Duration::from_secs(2),
            )
            .unwrap();
        });
        let connection = Tcp::connect(&format!(" {address} "), 1, Duration::from_secs(2)).unwrap();
        assert_eq!(connection.0.peer_addr().unwrap(), address);
        peer.join().unwrap();
    }
    #[test]
    fn fragmented_handshake_and_eof_mid_payload() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut hello = [0; 24];
            socket.read_exact(&mut hello).unwrap();
            assert_eq!(
                hello,
                [
                    65, 77, 66, 49, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
                ]
            );
            for byte in [
                65, 77, 66, 49, 1, 2, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ] {
                socket.write_all(&[byte]).unwrap();
            }
            socket
                .write_all(&[
                    65, 77, 66, 49, 1, 17, 0, 0, 2, 0, 0, 0, 5, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 9,
                    8,
                ])
                .unwrap();
        });
        let mut tcp = Tcp::connect("127.0.0.1", port, Duration::from_secs(2)).unwrap();
        assert!(
            Transport::receive(&mut tcp, Duration::from_secs(2))
                .unwrap_err()
                .to_string()
                .contains("disconnected")
        );
        peer.join().unwrap();
    }
    #[test]
    fn partial_header_and_payload_survive_poll_timeouts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = Tcp::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let frame = Frame::new(wire::PONG, 9, vec![1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
            let bytes = [frame.header.encode().unwrap().as_slice(), &frame.payload].concat();
            for chunk in bytes.chunks(8) {
                socket.write_all(chunk).unwrap();
                thread::sleep(Duration::from_millis(60));
            }
        });
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = loop {
            assert!(Instant::now() < deadline);
            match Transport::poll_receive(&mut client, Duration::from_millis(25)) {
                Ok(Some(frame)) => break frame,
                Ok(None) => {}
                Err(error) => panic!("poll lost frame boundary: {error}"),
            }
        };
        peer.join().unwrap();
        assert_eq!(result.header.kind, wire::PONG);
        assert_eq!(result.header.sequence, 9);
        assert_eq!(result.payload, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn header_and_payload_share_one_receive_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = Tcp::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let frame = Frame::new(wire::VIDEO_AU, 1, vec![42]).unwrap();
            thread::sleep(Duration::from_millis(120));
            socket.write_all(&frame.header.encode().unwrap()).unwrap();
            thread::sleep(Duration::from_millis(140));
            let _ = socket.write_all(&frame.payload);
        });
        let started = Instant::now();
        let outcome = Transport::receive(&mut client, Duration::from_millis(200));
        let elapsed = started.elapsed();
        drop(client);
        peer.join().unwrap();
        assert!(
            outcome.is_err(),
            "payload must not get a fresh 200ms budget"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "receive took {elapsed:?}"
        );
    }

    #[test]
    fn partial_reads_do_not_extend_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = Tcp::new(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
        let peer = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            for _ in 0..4 {
                let _ = socket.write_all(&[65]);
                thread::sleep(Duration::from_millis(35));
            }
        });
        let started = Instant::now();
        assert!(Transport::receive(&mut client, Duration::from_millis(75)).is_err());
        assert!(started.elapsed() < Duration::from_millis(130));
        peer.join().unwrap();
    }
}
