mod framing;
use crate::wire::{self, Frame};
pub(crate) use framing::FrameReader;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

pub trait Transport {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame>;
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()>;
    /// A poll timeout is not a frame boundary; implementations retain fragments.
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        self.receive(timeout).map(Some)
    }
}

pub struct Tcp(pub TcpStream, FrameReader);

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "TCP deadline expired"))
}

impl Tcp {
    pub fn new(socket: TcpStream) -> Self {
        Self(socket, FrameReader::default())
    }
    pub fn connect(host: &str, port: u16, timeout: Duration) -> crate::Result<Self> {
        if host.trim().is_empty() {
            return Err("enter an IP address".into());
        }
        let addresses: Vec<_> = match host.trim().parse::<SocketAddr>() {
            Ok(address) => vec![address],
            Err(_) => (host.trim(), port).to_socket_addrs()?.collect(),
        };
        let deadline = Instant::now() + timeout;
        let mut last = io::Error::new(io::ErrorKind::AddrNotAvailable, "no TCP addresses");
        for address in addresses {
            match TcpStream::connect_timeout(&address, remaining(deadline)?) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    let mut tcp = Self::new(stream);
                    tcp.send(
                        &Frame::new(wire::HELLO, 1, Vec::new())?,
                        remaining(deadline)?,
                    )?;
                    let ack = tcp.receive(remaining(deadline)?)?;
                    if ack.header.kind != wire::HELLO_ACK
                        || ack.header.sequence != 1
                        || !ack.payload.is_empty()
                    {
                        return Err("invalid HELLO_ACK".into());
                    }
                    return Ok(tcp);
                }
                Err(error) => last = error,
            }
        }
        Err(last.into())
    }
}

impl Transport for Tcp {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame> {
        self.poll_receive(timeout)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "receive poll expired").into())
    }
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        let socket = &mut self.0;
        self.1.poll(timeout, |bytes, left| {
            socket.set_read_timeout(Some(left))?;
            Ok(socket.read(bytes)?)
        })
    }
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()> {
        frame.validate()?;
        let deadline = Instant::now() + timeout;
        for bytes in [frame.header.encode()?.as_slice(), frame.payload.as_slice()] {
            let mut offset = 0;
            while offset < bytes.len() {
                self.0.set_write_timeout(Some(remaining(deadline)?))?;
                match self.0.write(&bytes[offset..]) {
                    Ok(0) => return Err("TCP write made no progress".into()),
                    Ok(n) => offset += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(())
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
            let hello = tcp.receive(Duration::from_secs(2)).unwrap();
            assert_eq!(hello.header.kind, wire::HELLO);
            tcp.send(
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
            tcp.receive(Duration::from_secs(2))
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
            match client.receive(Duration::from_millis(25)) {
                Ok(frame) => break frame,
                Err(error)
                    if error.downcast_ref::<io::Error>().is_some_and(|error| {
                        matches!(
                            error.kind(),
                            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                        )
                    }) => {}
                Err(error) => panic!("poll lost frame boundary: {error}"),
            }
        };
        peer.join().unwrap();
        assert_eq!(result.header.kind, wire::PONG);
        assert_eq!(result.header.sequence, 9);
        assert_eq!(result.payload, vec![1, 2, 3, 4, 5, 6, 7, 8]);
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
        assert!(client.receive(Duration::from_millis(75)).is_err());
        assert!(started.elapsed() < Duration::from_millis(130));
        peer.join().unwrap();
    }
}
