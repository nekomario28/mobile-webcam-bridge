//! AMB1 transports. Every connection speaks the same frame contract; the
//! socket framing, deadline handling and pairing exchange are owned here once
//! and shared by every `TcpStream`-backed transport.
use crate::wire::{self, Frame, Header};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

/// A bidirectional AMB1 conversation.
pub trait Transport {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame>;
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()>;
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
}

impl<T: SocketTransport> Transport for T {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        SocketTransport::receive(self, timeout)
    }
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        SocketTransport::send(self, frame, timeout)
    }
}

/// Time left on a deadline, never zero: some USB stacks read a zero timeout
/// as "wait forever".
pub(crate) fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .map(|duration| duration.max(Duration::from_millis(1)))
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "deadline expired"))
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
pub struct Tcp(pub TcpStream);

impl SocketTransport for Tcp {
    fn socket(&mut self) -> &mut TcpStream {
        &mut self.0
    }
    fn stream(&mut self) -> &mut dyn ByteStream {
        &mut self.0
    }
}

impl Tcp {
    pub fn connect(host: &str, port: u16, timeout: Duration) -> crate::Result<Self> {
        dial(host, port, timeout, |socket, deadline| {
            socket.set_nodelay(true)?;
            let mut tcp = Self(socket);
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
            let mut tcp = Tcp(socket);
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
    fn header_and_payload_share_one_receive_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = Tcp(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
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
        let mut client = Tcp(TcpStream::connect(listener.local_addr().unwrap()).unwrap());
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
