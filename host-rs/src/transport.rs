use crate::wire::{self, Frame, Header};
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};

pub trait Transport {
    fn receive(&mut self, timeout: Duration) -> crate::Result<Frame>;
    fn send(&mut self, frame: &Frame, timeout: Duration) -> crate::Result<()>;
}

pub struct Tcp(pub TcpStream);

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "TCP deadline expired"))
}

impl Tcp {
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
                    let mut tcp = Self(stream);
                    tcp.send(&Frame::new(wire::HELLO, 1, Vec::new())?, timeout)?;
                    let ack = tcp.receive(timeout)?;
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

    fn read_exact_deadline(&mut self, bytes: &mut [u8], timeout: Duration) -> io::Result<()> {
        let deadline = Instant::now() + timeout;
        let mut offset = 0;
        while offset < bytes.len() {
            self.0.set_read_timeout(Some(remaining(deadline)?))?;
            match self.0.read(&mut bytes[offset..]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "TCP peer disconnected",
                    ));
                }
                Ok(n) => offset += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl Transport for Tcp {
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
            let mut tcp = Tcp(socket);
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
        assert!(client.receive(Duration::from_millis(75)).is_err());
        assert!(started.elapsed() < Duration::from_millis(130));
        peer.join().unwrap();
    }
}
