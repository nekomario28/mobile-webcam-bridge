//! The PC's half of the per-PC authorization exchange
//! ([design](../../docs/per-pc-authorization.md)).
//!
//! The phone sends a fresh nonce; this PC proves possession of its own key over
//! `clientId || nonce || label`; the phone answers. Nothing here decides
//! anything — the phone owns the list — and nothing retries: a rejection is
//! reported and the user decides.
//!
//! Not wired into the connect path yet. It is implemented and tested against a
//! peer that plays the phone, so the exchange is proven before it is trusted.

use crate::{
    identity::Identity,
    transport::{SocketTransport, remaining},
    wire::{self, Frame},
};
use std::time::{Duration, Instant};

/// What the phone said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Authorized until this Unix second.
    Authorized { expires_at_unix_seconds: u32 },
    /// Registered but not yet approved by a tap on the phone.
    Pending,
    /// Rejected, with the phone's own reason as text.
    Rejected(String),
}

/// The label this PC proposes. The phone displays it and must not trust it, so
/// it is a convenience name and never an identity.
pub fn proposed_label() -> String {
    for variable in ["HOSTNAME", "COMPUTERNAME", "USER"] {
        if let Ok(value) = std::env::var(variable) {
            let trimmed = value.trim();
            if !trimmed.is_empty() && trimmed.len() <= wire::MAX_CLIENT_LABEL {
                return trimmed.to_string();
            }
        }
    }
    "this PC".to_string()
}

fn reason_text(reason: u8) -> String {
    match reason {
        wire::AUTH_REASON_UNKNOWN => "this PC is not known to the phone".to_string(),
        wire::AUTH_REASON_REVOKED => "this PC's access was revoked on the phone".to_string(),
        wire::AUTH_REASON_EXPIRED => "this PC's authorization expired on the phone".to_string(),
        wire::AUTH_REASON_TOO_MANY => "the phone already authorizes too many PCs".to_string(),
        wire::AUTH_REASON_BAD_SIGNATURE => "the phone rejected this PC's signature".to_string(),
        _ => "the phone rejected this PC".to_string(),
    }
}

/// Run the exchange over an already-paired transport.
///
/// `timeout` is one budget for the whole exchange, not per frame: a peer that
/// trickles a challenge must not be able to extend it.
pub fn exchange(
    transport: &mut impl SocketTransport,
    identity: &Identity,
    timeout: Duration,
) -> crate::Result<Outcome> {
    let deadline = Instant::now() + timeout;
    let challenge = transport.receive_until(deadline)?;
    if challenge.header.kind != wire::CLIENT_CHALLENGE {
        return Err(format!(
            "expected a client challenge, received type 0x{:02x}",
            challenge.header.kind
        )
        .into());
    }
    let challenge = wire::ClientChallenge::decode(&challenge.payload)?;
    let label = proposed_label();
    let client_id = identity.client_id().to_vec();
    let nonce = challenge.nonce.clone();
    let message: Vec<u8> = client_id
        .iter()
        .chain(nonce.iter())
        .chain(label.as_bytes())
        .copied()
        .collect();
    let signature = identity.sign(&message)?;
    transport.send_until(
        &Frame::new(
            wire::CLIENT_AUTH,
            0,
            wire::ClientAuth {
                version: 1,
                client_id,
                label,
                signature: signature.to_vec(),
            }
            .encode(),
        )?,
        deadline,
    )?;
    let result = transport.receive_until(deadline)?;
    if result.header.kind != wire::CLIENT_AUTH_RESULT {
        return Err(format!(
            "expected a client auth result, received type 0x{:02x}",
            result.header.kind
        )
        .into());
    }
    let result = wire::ClientAuthResult::decode(&result.payload)?;
    Ok(match result.status {
        wire::AUTH_AUTHORIZED => Outcome::Authorized {
            expires_at_unix_seconds: result.expires_at_unix_seconds,
        },
        wire::AUTH_PENDING => Outcome::Pending,
        _ => Outcome::Rejected(reason_text(result.reason)),
    })
}

/// The deadline rule is the shared one: an expired budget fails the next
/// operation rather than discarding a delivered frame.
pub fn remaining_until(deadline: Instant) -> std::io::Result<Duration> {
    remaining(deadline)
}

#[cfg(test)]
mod tests;
