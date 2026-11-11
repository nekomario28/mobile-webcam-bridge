//! Memory soak: a long synthetic stream and many reconnects over the real TLS
//! transport, with the process's resident set sampled over time.
//!
//! Ignored by default; the scheduled `soak` workflow runs it:
//! `SOAK_SECONDS=600 cargo test --release --no-default-features --test soak -- --ignored --nocapture`
//!
//! What it catches: per-frame or per-connection growth in the receive path
//! (TLS records, frame reassembly, payload buffers, deadline wrappers). What it
//! does not: the FFmpeg decoder and the virtual-camera output, which need the
//! `video` feature and a device; `tools/phone-soak.sh` covers the real phone.
#![cfg(target_os = "linux")]
mod common;

use common::{ACCESS_KEY, PHONE_SHA256, phone_tls};
use mobile_webcam::{
    secure::SecureTcp,
    transport::Transport,
    wire::{self, Frame},
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// Resident set size of this process in bytes, from `/proc/self/statm`.
fn rss_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").expect("procfs");
    let pages: u64 = statm.split_whitespace().nth(1).unwrap().parse().unwrap();
    // SAFETY: sysconf has no preconditions.
    pages * unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as u64
}

fn soak_seconds() -> u64 {
    std::env::var("SOAK_SECONDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(30)
        .max(10)
}

/// The phone side of HELLO: read the client's credential, answer HELLO_ACK.
fn accept_hello(tls: &mut (impl Read + Write)) {
    let mut header = [0u8; wire::HEADER_SIZE];
    tls.read_exact(&mut header).unwrap();
    let hello = wire::Header::decode(&header).unwrap();
    assert_eq!(hello.kind, wire::HELLO);
    let mut credential = vec![0u8; hello.payload_len as usize];
    tls.read_exact(&mut credential).unwrap();
    let ack = Frame::new(wire::HELLO_ACK, hello.sequence, Vec::new()).unwrap();
    tls.write_all(&ack.header.encode().unwrap()).unwrap();
    tls.flush().unwrap();
}

fn send(tls: &mut impl Write, frame: &Frame) -> std::io::Result<()> {
    tls.write_all(&frame.header.encode().unwrap())?;
    tls.write_all(&frame.payload)?;
    Ok(())
}

/// Xorshift: deterministic, varied frame sizes without a dependency.
struct Sizes(u64);
impl Sizes {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        // Mostly P-frame sized, now and then an IDR-sized one.
        if self.0.is_multiple_of(30) {
            150_000 + (self.0 % 100_000) as usize
        } else {
            2_000 + (self.0 % 30_000) as usize
        }
    }
}

/// Median of the samples' second half against the first post-warm-up sample.
fn report(name: &str, samples: &[(Duration, u64)], allowed_growth: u64) {
    assert!(samples.len() >= 4, "{name}: too few samples");
    for (at, rss) in samples {
        println!("{name}: t={:>5}s rss={:>8} KiB", at.as_secs(), rss / 1024);
    }
    let baseline = samples[0].1;
    let mut tail: Vec<u64> = samples[samples.len() / 2..].iter().map(|s| s.1).collect();
    tail.sort_unstable();
    let median = tail[tail.len() / 2];
    let growth = median.saturating_sub(baseline);
    println!(
        "{name}: baseline {} KiB, late median {} KiB, growth {} KiB (allowed {} KiB)",
        baseline / 1024,
        median / 1024,
        growth / 1024,
        allowed_growth / 1024
    );
    assert!(
        growth <= allowed_growth,
        "{name}: resident set grew by {} KiB",
        growth / 1024
    );
}

#[test]
#[ignore = "long-running; run by the scheduled soak workflow"]
fn a_long_stream_does_not_grow_the_receive_path() {
    let seconds = soak_seconds();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let phone_stop = stop.clone();
    let phone = thread::spawn(move || {
        let mut tls = phone_tls(listener.accept().unwrap().0);
        accept_hello(&mut tls);
        let mut sizes = Sizes(0x2545_f491_4f6c_dd1d);
        let mut sequence = 1u32;
        while !phone_stop.load(Ordering::Relaxed) {
            let mut frame = Frame::new(wire::VIDEO_AU, sequence, vec![0x5a; sizes.next()]).unwrap();
            frame.header.flags = if sequence % 30 == 1 {
                wire::KEYFRAME
            } else {
                0
            };
            if send(&mut tls, &frame).is_err() {
                return;
            }
            if sequence.is_multiple_of(60) {
                let ping =
                    Frame::new(wire::PING, sequence, sequence.to_le_bytes().repeat(2)).unwrap();
                if send(&mut tls, &ping).is_err() {
                    return;
                }
            }
            let _ = tls.flush();
            sequence = sequence.wrapping_add(1);
        }
    });

    let mut client = SecureTcp::connect(
        "127.0.0.1",
        port,
        PHONE_SHA256,
        ACCESS_KEY,
        Duration::from_secs(4),
    )
    .unwrap();
    let start = Instant::now();
    let warm_up = Duration::from_secs(seconds / 10).max(Duration::from_secs(3));
    let interval = Duration::from_secs((seconds / 20).max(1));
    let mut next_sample = start + warm_up;
    let mut samples = Vec::new();
    let mut frames = 0u64;
    let mut bytes = 0u64;
    while start.elapsed() < Duration::from_secs(seconds) {
        let frame = client.receive(Duration::from_secs(5)).unwrap();
        frames += 1;
        bytes += frame.payload.len() as u64;
        if Instant::now() >= next_sample {
            samples.push((start.elapsed(), rss_bytes()));
            next_sample += interval;
        }
    }
    stop.store(true, Ordering::Relaxed);
    drop(client);
    phone.join().unwrap();
    println!(
        "stream: {frames} frames, {} MiB in {seconds}s",
        bytes / (1024 * 1024)
    );
    assert!(frames > 1_000, "the synthetic stream barely ran");
    report("stream", &samples, 8 * 1024 * 1024);
}

#[test]
#[ignore = "long-running; run by the scheduled soak workflow"]
fn repeated_connections_do_not_accumulate() {
    let seconds = soak_seconds();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let phone_stop = stop.clone();
    let phone = thread::spawn(move || {
        while !phone_stop.load(Ordering::Relaxed) {
            let Ok((socket, _)) = listener.accept() else {
                return;
            };
            if phone_stop.load(Ordering::Relaxed) {
                return;
            }
            let mut tls = phone_tls(socket);
            accept_hello(&mut tls);
            for sequence in 1..=20u32 {
                let frame = Frame::new(wire::VIDEO_AU, sequence, vec![0xa5; 8_000]).unwrap();
                if send(&mut tls, &frame).is_err() {
                    break;
                }
            }
            let _ = tls.flush();
        }
    });

    let start = Instant::now();
    let warm_up = Duration::from_secs(seconds / 10).max(Duration::from_secs(3));
    let interval = Duration::from_secs((seconds / 20).max(1));
    let mut next_sample = start + warm_up;
    let mut samples = Vec::new();
    let mut sessions = 0u64;
    while start.elapsed() < Duration::from_secs(seconds) {
        let mut client = SecureTcp::connect(
            "127.0.0.1",
            port,
            PHONE_SHA256,
            ACCESS_KEY,
            Duration::from_secs(4),
        )
        .unwrap();
        for _ in 0..20 {
            client.receive(Duration::from_secs(5)).unwrap();
        }
        drop(client);
        sessions += 1;
        if Instant::now() >= next_sample {
            samples.push((start.elapsed(), rss_bytes()));
            next_sample += interval;
        }
    }
    stop.store(true, Ordering::Relaxed);
    // Wake the accept loop so the phone thread can see `stop`.
    let _ = std::net::TcpStream::connect(("127.0.0.1", port));
    phone.join().unwrap();
    println!("reconnect: {sessions} sessions in {seconds}s");
    assert!(sessions > 50, "too few sessions to say anything");
    report("reconnect", &samples, 4 * 1024 * 1024);
}
