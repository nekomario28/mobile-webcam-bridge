//! The PC's half of the per-PC authorization exchange
//! ([design](../../docs/per-pc-authorization.md)).
//!
//! The phone sends a fresh nonce; this PC proves possession of its own key over
//! a domain-separated v2 transcript including transport, phone binding, nonce,
//! point and label. The phone owns grants; a rejection is terminal, not retried.
//! Both production connection paths call this before opening video/output.
//! Pending notifies the UI but never substitutes for a separate Authorized
//! outcome. Tests exercise mechanics, not independent security approval.

use crate::{
    identity::Identity,
    transport::remaining,
    wire::{self, Frame},
};
use std::time::{Duration, Instant};

/// LAN proofs must name the certificate this connection actually pinned.
/// USB has no cryptographic phone attestation; zero binding is explicit.
#[derive(Clone, Copy, Debug)]
pub enum Route {
    Usb,
    Lan {
        phone_sha256: [u8; wire::PHONE_BINDING_BYTES],
    },
}
impl Route {
    fn transport(self) -> u8 {
        match self {
            Self::Usb => wire::AUTH_USB,
            Self::Lan { .. } => wire::AUTH_LAN,
        }
    }
    fn phone_binding(self) -> [u8; wire::PHONE_BINDING_BYTES] {
        match self {
            Self::Usb => [0; wire::PHONE_BINDING_BYTES],
            Self::Lan { phone_sha256 } => phone_sha256,
        }
    }
}

/// What the phone said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Authorized until this Unix second.
    Authorized { expires_at_unix_seconds: u32 },
    /// Rejected, with the phone's own reason as text.
    Rejected(String),
}

/// The label this PC proposes. The phone displays it and must not trust it, so
/// it is a convenience name and never an identity.
pub fn proposed_label() -> String {
    for variable in ["HOSTNAME", "COMPUTERNAME", "USER"] {
        if let Ok(value) = std::env::var(variable) {
            let trimmed = value.trim();
            // OS names can contain invisible formatting. The automatic label
            // stays printable ASCII; a future editable name can use the phone's
            // stricter Unicode policy instead of signing ambiguous UI text.
            if !trimmed.is_empty()
                && trimmed.len() <= wire::MAX_CLIENT_LABEL
                && trimmed.bytes().all(|c| (0x20..=0x7e).contains(&c))
            {
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

/// Run the mandatory exchange over TLS-paired LAN or a physical USB stream.
///
/// `timeout` is one budget for the whole exchange, not per frame: a peer that
/// trickles a challenge must not be able to extend it. One Pending outcome
/// starts a separate bounded human-decision interval; polls remain stoppable.
pub fn exchange(
    transport: &mut dyn crate::transport::Transport,
    identity: &Identity,
    route: Route,
    timeout: Duration,
    stop: &std::sync::atomic::AtomicBool,
    mut on_pending: impl FnMut() -> crate::Result<()>,
) -> crate::Result<Outcome> {
    let mut deadline = Instant::now() + timeout;
    let challenge = receive_until(transport, stop, deadline)?;
    if challenge.header.kind != wire::CLIENT_CHALLENGE {
        return Err("phone does not support required per-PC authorization v2".into());
    }
    let challenge = wire::ClientChallenge::decode(&challenge.payload)?;
    if challenge.transport != route.transport() {
        return Err("authorization transport binding mismatch".into());
    }
    if challenge.phone_binding != route.phone_binding() {
        return Err("authorization phone certificate binding mismatch".into());
    }
    let label = proposed_label();
    let message = wire::client_auth_message(
        route.transport(),
        &route.phone_binding(),
        &challenge.nonce,
        identity.public_key(),
        &label,
    )?;
    let signature = identity.sign(&message)?;
    transport.send(
        &Frame::new(
            wire::CLIENT_AUTH,
            0,
            wire::ClientAuth {
                version: wire::CLIENT_AUTH_VERSION,
                public_key: identity.public_key().to_vec(),
                label,
                signature: signature.to_vec(),
            }
            .encode(),
        )?,
        remaining_until(deadline)?.min(Duration::from_secs(2)),
    )?;

    let mut pending = false;
    loop {
        let frame = receive_until(transport, stop, deadline)?;
        if frame.header.kind != wire::CLIENT_AUTH_RESULT {
            return Err("expected authorization outcome before capabilities/media".into());
        }
        let result = wire::ClientAuthResult::decode(&frame.payload)?;
        match result.status {
            wire::AUTH_AUTHORIZED => {
                return Ok(Outcome::Authorized {
                    expires_at_unix_seconds: result.expires_at_unix_seconds,
                });
            }
            wire::AUTH_REJECTED => return Ok(Outcome::Rejected(reason_text(result.reason))),
            wire::AUTH_PENDING if !pending => {
                pending = true;
                // One human-decision interval, never renewed by another frame.
                deadline = Instant::now() + Duration::from_secs(60);
                on_pending()?;
            }
            _ => return Err("repeated pending authorization outcome".into()),
        }
    }
}

fn receive_until(
    transport: &mut dyn crate::transport::Transport,
    stop: &std::sync::atomic::AtomicBool,
    deadline: Instant,
) -> crate::Result<Frame> {
    while !stop.load(std::sync::atomic::Ordering::Acquire) {
        if let Some(frame) =
            transport.poll_receive(remaining_until(deadline)?.min(Duration::from_millis(100)))?
        {
            return Ok(frame);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "authorization stopped").into())
}

/// The deadline rule is the shared one: an expired budget fails the next
/// operation rather than discarding a delivered frame.
pub fn remaining_until(deadline: Instant) -> std::io::Result<Duration> {
    remaining(deadline)
}

#[cfg(test)]
mod tests;
