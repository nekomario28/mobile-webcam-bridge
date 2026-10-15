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
            version: 1,
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
    assert_eq!(auth.version, 1);
    assert_eq!(auth.label, expect_label);
    // The peer verifies against the key the test handed it, which is what the
    // phone stores. Passed in rather than shared, because these tests run in
    // parallel and a static would be another test's key.
    assert!(Identity::verify(
        &public_key,
        &signed(&auth.client_id, &nonce, &auth.label),
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
            expires_at_unix_seconds: 1_800_000_000,
        }
        .encode(),
    )
    .unwrap();
    tls.write_all(&result.header.encode().unwrap()).unwrap();
    tls.write_all(&result.payload).unwrap();
    tls.flush().unwrap();
    // Wait for the client to close after consuming the result. A timeout is
    // not successful teardown: the owner must drop its transport before join.
    let mut byte = [0u8];
    match tls.read(&mut byte) {
        Ok(0) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
        other => panic!("client did not disconnect after reading auth result: {other:?}"),
    }
}

fn signed(client_id: &[u8], nonce: &[u8], label: &str) -> Vec<u8> {
    client_id
        .iter()
        .chain(nonce.iter())
        .chain(label.as_bytes())
        .copied()
        .collect()
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
    let outcome = exchange(&mut transport, &identity, Duration::from_secs(5))
        .expect("the exchange completes");
    drop(transport);
    peer.join().expect("the peer finishes");
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
fn an_unapproved_client_is_pending_not_an_error() {
    assert_eq!(run([9u8; 32], Reply::Pending), Outcome::Pending);
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

/// A nonce that is not what the phone sent must not verify: the signature
/// covers it, so a replay against a different challenge fails at the peer.
#[test]
fn the_signature_covers_the_nonce() {
    let identity = Identity::generate();
    let nonce = [1u8; 32];
    let label = "studio-pc";
    let client_id = identity.client_id().to_vec();
    let message = signed(&client_id, &nonce, label);
    let signature = identity.sign(&message).unwrap();
    assert!(Identity::verify(
        identity.public_key(),
        &message,
        &signature
    ));
    let other = signed(&client_id, &[2u8; 32], label);
    assert!(!Identity::verify(identity.public_key(), &other, &signature));
}

/// A label the PC did not sign cannot be swapped in.
#[test]
fn the_signature_covers_the_label() {
    let identity = Identity::generate();
    let nonce = [4u8; 32];
    let client_id = identity.client_id().to_vec();
    let signature = identity.sign(&signed(&client_id, &nonce, "real")).unwrap();
    assert!(!Identity::verify(
        identity.public_key(),
        &signed(&client_id, &nonce, "fake"),
        &signature
    ));
}
