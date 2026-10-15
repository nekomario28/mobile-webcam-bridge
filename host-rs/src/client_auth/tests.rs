//! The exchange tested against a peer that plays the phone, over real TLS.
//!
//! The peer is a `StreamOwned<ServerConnection, TcpStream>` from the shared
//! fixture, so the PC side is exercised through the same transport it will use
//! in production rather than a mock.

use super::*;
use crate::{identity::Identity, secure::SecureTcp, wire};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

// The same phone-side TLS fixture the integration tests use, so this suite
// exercises the same transport the production path will. It is also a module of
// those tests, which is why the lint about a file loaded twice is expected here.
#[path = "../../tests/common/mod.rs"]
#[allow(clippy::duplicate_mod)]
mod common;

use common::{ACCESS_KEY, PHONE_SHA256, phone_tls};

/// What the phone peer will answer, and the nonce it will challenge with.
#[derive(Clone, Copy)]
enum Reply {
    Authorized,
    Pending,
    Rejected(u8),
}

fn phone_peer(
    listener: TcpListener,
    nonce: [u8; 32],
    reply: Reply,
    expect_label: &str,
    public_key: Vec<u8>,
) {
    let mut tls = phone_tls(listener.accept().expect("accept").0);
    let mut header = [0u8; wire::HEADER_SIZE];
    tls.read_exact(&mut header).unwrap();
    assert_eq!(&header[..4], b"AMB1");
    assert_eq!(header[5], wire::HELLO);
    let credential = u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize;
    tls.read_exact(&mut vec![0u8; credential][..]).unwrap();

    // Answer pairing, then challenge with a known nonce.
    let mut ack = header;
    ack[5] = wire::HELLO_ACK;
    ack[12..16].copy_from_slice(&0u32.to_le_bytes());
    tls.write_all(&ack).unwrap();

    let challenge = wire::Frame::new(
        wire::CLIENT_CHALLENGE,
        0,
        wire::ClientChallenge {
            version: wire::CLIENT_AUTH_VERSION,
            transport: wire::AUTH_LAN,
            phone_binding: common::phone_binding(),
            nonce: nonce.to_vec(),
        }
        .encode(),
    )
    .unwrap();
    tls.write_all(&challenge.header.encode().unwrap()).unwrap();
    tls.write_all(&challenge.payload).unwrap();
    tls.flush().unwrap();

    // Read the PC's proof and verify it the way the phone would.
    let mut header = [0u8; wire::HEADER_SIZE];
    tls.read_exact(&mut header).unwrap();
    assert_eq!(header[5], wire::CLIENT_AUTH);
    let mut payload = vec![0u8; u32::from_le_bytes(header[12..16].try_into().unwrap()) as usize];
    tls.read_exact(&mut payload).unwrap();
    let auth = wire::ClientAuth::decode(&payload).unwrap();
    assert_eq!(auth.version, wire::CLIENT_AUTH_VERSION);
    assert_eq!(auth.public_key, public_key);
    assert_eq!(auth.label, expect_label);
    // The peer verifies against the key the test handed it, which is what the
    // phone stores. Passed in rather than shared, because these tests run in
    // parallel and a static would be another test's key.
    assert!(Identity::verify(
        &public_key,
        &signed(&auth.public_key, &nonce, &auth.label),
        &auth.signature
    ));

    let result = wire::Frame::new(
        wire::CLIENT_AUTH_RESULT,
        0,
        wire::ClientAuthResult {
            status: match reply {
                Reply::Authorized => wire::AUTH_AUTHORIZED,
                Reply::Pending => wire::AUTH_PENDING,
                Reply::Rejected(_) => wire::AUTH_REJECTED,
            },
            reason: match reply {
                Reply::Rejected(reason) => reason,
                _ => wire::AUTH_REASON_NONE,
            },
            expires_at_unix_seconds: if matches!(
                reply,
                Reply::Authorized | Reply::Rejected(wire::AUTH_REASON_EXPIRED)
            ) {
                1_800_000_000
            } else {
                0
            },
        }
        .encode(),
    )
    .unwrap();
    tls.write_all(&result.header.encode().unwrap()).unwrap();
    tls.write_all(&result.payload).unwrap();
    tls.flush().unwrap();
    if matches!(reply, Reply::Pending) {
        // Explicit fixture-side approval is a second outcome, never inferred
        // from Pending by the host.
        let granted = wire::ClientAuthResult {
            status: wire::AUTH_AUTHORIZED,
            reason: wire::AUTH_REASON_NONE,
            expires_at_unix_seconds: 1_800_000_000,
        }
        .encode();
        tls.write_all(&result.header.encode().unwrap()).unwrap();
        tls.write_all(&granted).unwrap();
        tls.flush().unwrap();
    }
    // Wait for the client to close after consuming the result. A timeout is
    // not successful teardown: the owner must drop its transport before join.
    let mut byte = [0u8];
    match tls.read(&mut byte) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
        other => panic!("client did not disconnect after reading auth result: {other:?}"),
    }
}

fn lan_route() -> Route {
    Route::Lan {
        phone_sha256: common::phone_binding().as_slice().try_into().unwrap(),
    }
}
fn signed(public_key: &[u8], nonce: &[u8], label: &str) -> Vec<u8> {
    wire::client_auth_message(
        wire::AUTH_LAN,
        &common::phone_binding(),
        nonce,
        public_key,
        label,
    )
    .unwrap()
}

fn run(nonce: [u8; 32], reply: Reply) -> Outcome {
    let identity = Identity::generate();
    let public_key = identity.public_key().to_vec();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let label = proposed_label();
    let peer = thread::spawn(move || {
        phone_peer(listener, nonce, reply, &label, public_key);
    });
    let mut transport = SecureTcp::connect(
        "127.0.0.1",
        port,
        PHONE_SHA256,
        ACCESS_KEY,
        Duration::from_secs(5),
    )
    .expect("the peer completes pairing");
    let mut pending_count = 0;
    let outcome = exchange(
        &mut transport,
        &identity,
        lan_route(),
        Duration::from_secs(5),
        &std::sync::atomic::AtomicBool::new(false),
        || {
            pending_count += 1;
            Ok(())
        },
    )
    .expect("the exchange completes");
    drop(transport);
    peer.join().expect("the peer finishes");
    assert_eq!(pending_count, usize::from(matches!(reply, Reply::Pending)));
    outcome
}

#[test]
fn an_authorized_client_gets_its_expiry() {
    assert_eq!(
        run([7u8; 32], Reply::Authorized),
        Outcome::Authorized {
            expires_at_unix_seconds: 1_800_000_000
        }
    );
}

#[test]
fn pending_notifies_the_ui_and_waits_for_an_explicit_authorized_outcome() {
    assert_eq!(
        run([9u8; 32], Reply::Pending),
        Outcome::Authorized {
            expires_at_unix_seconds: 1_800_000_000
        }
    );
}

#[test]
fn each_rejection_reason_reaches_the_user_as_text() {
    for (reason, expected) in [
        (wire::AUTH_REASON_UNKNOWN, "not known to the phone"),
        (wire::AUTH_REASON_REVOKED, "revoked"),
        (wire::AUTH_REASON_EXPIRED, "expired"),
        (wire::AUTH_REASON_TOO_MANY, "too many PCs"),
        (wire::AUTH_REASON_BAD_SIGNATURE, "signature"),
    ] {
        let outcome = run([3u8; 32], Reply::Rejected(reason));
        assert!(
            matches!(outcome, Outcome::Rejected(ref text) if text.contains(expected)),
            "{reason}: {outcome:?}"
        );
    }
}

/// A minimal transport for sequencing/Stop assertions. It must never need a
/// long blocking receive, and it never fabricates an authorization outcome.
struct Script {
    frames: std::collections::VecDeque<Frame>,
    sent: Vec<u8>,
}
impl crate::transport::Transport for Script {
    fn receive(&mut self, _: Duration) -> crate::Result<Frame> {
        panic!("authorization must poll for Stop")
    }
    fn poll_receive(&mut self, timeout: Duration) -> crate::Result<Option<Frame>> {
        assert!(timeout <= Duration::from_millis(100));
        Ok(Some(
            self.frames.pop_front().expect("unexpected extra auth poll"),
        ))
    }
    fn send(&mut self, frame: &Frame, _: Duration) -> crate::Result<()> {
        self.sent.push(frame.header.kind);
        Ok(())
    }
}
fn scripted(outcomes: &[u8], transport: u8) -> Script {
    let challenge = Frame::new(
        wire::CLIENT_CHALLENGE,
        0,
        wire::ClientChallenge {
            version: wire::CLIENT_AUTH_VERSION,
            transport,
            phone_binding: if transport == wire::AUTH_USB {
                vec![0; 32]
            } else {
                common::phone_binding()
            },
            nonce: vec![3; 32],
        }
        .encode(),
    )
    .unwrap();
    let mut frames = std::collections::VecDeque::from([challenge]);
    for status in outcomes {
        frames.push_back(
            Frame::new(
                wire::CLIENT_AUTH_RESULT,
                0,
                wire::ClientAuthResult {
                    status: *status,
                    reason: wire::AUTH_REASON_NONE,
                    expires_at_unix_seconds: if *status == wire::AUTH_AUTHORIZED {
                        1_800_000_000
                    } else {
                        0
                    },
                }
                .encode(),
            )
            .unwrap(),
        );
    }
    Script {
        frames,
        sent: vec![],
    }
}

#[test]
fn usb_uses_the_same_mandatory_exchange_without_a_tls_or_qr_identity() {
    let mut script = scripted(&[wire::AUTH_AUTHORIZED], wire::AUTH_USB);
    let outcome = exchange(
        &mut script,
        &Identity::generate(),
        Route::Usb,
        Duration::from_secs(5),
        &std::sync::atomic::AtomicBool::new(false),
        || panic!("known USB PC should not need a new approval"),
    )
    .unwrap();
    assert_eq!(
        outcome,
        Outcome::Authorized {
            expires_at_unix_seconds: 1_800_000_000
        }
    );
    assert_eq!(script.sent, vec![wire::CLIENT_AUTH]);
}

#[test]
fn stop_while_pending_never_consumes_a_later_grant_or_starts_streaming() {
    let mut script = scripted(&[wire::AUTH_PENDING, wire::AUTH_AUTHORIZED], wire::AUTH_LAN);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut notices = 0;
    let error = exchange(
        &mut script,
        &Identity::generate(),
        lan_route(),
        Duration::from_secs(5),
        &stop,
        || {
            notices += 1;
            stop.store(true, std::sync::atomic::Ordering::Release);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("stopped"));
    assert_eq!(notices, 1);
    assert_eq!(script.frames.len(), 1);
    assert_eq!(script.sent, vec![wire::CLIENT_AUTH]);
}

#[test]
fn repeated_pending_is_a_protocol_error_not_a_renewable_timeout() {
    let mut script = scripted(&[wire::AUTH_PENDING, wire::AUTH_PENDING], wire::AUTH_LAN);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut notices = 0;
    let error = exchange(
        &mut script,
        &Identity::generate(),
        lan_route(),
        Duration::from_secs(5),
        &stop,
        || {
            notices += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("repeated pending"));
    assert_eq!(notices, 1);
}

#[test]
fn transport_mismatch_fails_before_signing_and_old_phone_frames_never_fallback() {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let identity = Identity::generate();
    let mut wrong = scripted(&[], wire::AUTH_USB);
    let error = exchange(
        &mut wrong,
        &identity,
        lan_route(),
        Duration::from_secs(5),
        &stop,
        || panic!("mismatched transport reached UI approval"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("binding mismatch"));
    assert!(wrong.sent.is_empty());
    let mut old = Script {
        frames: std::collections::VecDeque::from([
            Frame::new(wire::VIDEO_CONFIG, 0, vec![0; 16]).unwrap()
        ]),
        sent: vec![],
    };
    let error = exchange(
        &mut old,
        &identity,
        lan_route(),
        Duration::from_secs(5),
        &stop,
        || panic!("old phone reached UI approval"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("required per-PC authorization"));
    assert!(old.sent.is_empty());
}

#[test]
fn the_signature_binds_phone_transport_and_public_point_not_only_nonce() {
    let identity = Identity::generate();
    let signature = identity
        .sign(&signed(identity.public_key(), &[3; 32], "pc"))
        .unwrap();
    let usb = wire::client_auth_message(
        wire::AUTH_USB,
        &[0; 32],
        &[3; 32],
        identity.public_key(),
        "pc",
    )
    .unwrap();
    assert!(!Identity::verify(identity.public_key(), &usb, &signature));
    let wrong_phone = wire::client_auth_message(
        wire::AUTH_LAN,
        &[0x7f; 32],
        &[3; 32],
        identity.public_key(),
        "pc",
    )
    .unwrap();
    assert!(!Identity::verify(
        identity.public_key(),
        &wrong_phone,
        &signature
    ));
    let other = Identity::generate();
    let changed_point = signed(other.public_key(), &[3; 32], "pc");
    assert!(!Identity::verify(
        identity.public_key(),
        &changed_point,
        &signature
    ));
}

#[test]
fn another_pinned_phone_cannot_obtain_a_proof_for_this_phone() {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let mut script = scripted(&[], wire::AUTH_LAN);
    script.frames.front_mut().unwrap().payload[2..34].fill(0x7f);
    let error = exchange(
        &mut script,
        &Identity::generate(),
        lan_route(),
        Duration::from_secs(5),
        &stop,
        || panic!("wrong phone reached approval"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("phone certificate binding mismatch")
    );
    assert!(
        script.sent.is_empty(),
        "never sign a different phone's challenge"
    );
}

/// A nonce that is not what the phone sent must not verify: the signature
/// covers it, so a replay against a different challenge fails at the peer.
#[test]
fn the_signature_covers_the_nonce() {
    let identity = Identity::generate();
    let nonce = [1u8; 32];
    let label = "studio-pc";
    let public_key = identity.public_key().to_vec();
    let message = signed(&public_key, &nonce, label);
    let signature = identity.sign(&message).unwrap();
    assert!(Identity::verify(
        identity.public_key(),
        &message,
        &signature
    ));
    let other = signed(&public_key, &[2u8; 32], label);
    assert!(!Identity::verify(identity.public_key(), &other, &signature));
}

/// A label the PC did not sign cannot be swapped in.
#[test]
fn the_signature_covers_the_label() {
    let identity = Identity::generate();
    let nonce = [4u8; 32];
    let public_key = identity.public_key().to_vec();
    let signature = identity.sign(&signed(&public_key, &nonce, "real")).unwrap();
    assert!(!Identity::verify(
        identity.public_key(),
        &signed(&public_key, &nonce, "fake"),
        &signature
    ));
}
