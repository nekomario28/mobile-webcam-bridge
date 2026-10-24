# AMB1 per-PC authorization v2

**Unreviewed integration protocol, not independently security-approved.** This
replaces unshipped v1 scaffolding outright; there is no v1 compatibility path.
AMB1 framing remains version 1. Integers below are little-endian unless stated.

## Placement and authority

Wi-Fi first completes pinned TLS 1.3 and access-key HELLO/HELLO_ACK as in
[secure LAN](wire-secure-lan.md). USB starts at the challenge without a QR or
HELLO credential. Both then follow the same mandatory authorization exchange.
### Host hello: who speaks first on a reconnect (2026-10-24)

USB has no session boundary and AOA reports no disconnect, so a host that reconnects finds
the accessory session the previous connection used — quiet (it authorised somebody) or still
answering with the older connection's frames. Neither looks like a live peer, and the only
way to get a fresh exchange used to be restarting the app or replugging the cable.

`HOST_HELLO` (`0x0e`, zero payload) is the host saying "a host is here". The phone answers:

- **waiting for a proof** — re-send the *same* challenge (same nonce), because the peer may
  have read a challenge this session already sent and lost it;
- **already answered** (authorised, pending or denied) — end the session, so the next host
  gets a fresh challenge;
- **not yet challenged** — ignore; a challenge is on its way.

It carries no identity and grants nothing, so it is safe to send to a phone that does not
know this protocol yet: an app that does not handle the type treats it as an unexpected
control frame, ends that session and rediscovers — which is the behaviour the hello exists
to produce. The host sends it only after a failure that a spent session explains (a silent
budget, a stream that will not parse, a first frame that is not a challenge), then retries
the exchange once; a *rejection* is an answer and is reported as one.

The phone sends no capabilities/media and accepts no quality/PING/IDR controls
until Authorized. Unsupported peers close, never fall back to bearer-only access.

The phone owns a random 32-byte nonce tied to one live physical session. It
accepts exactly one proof; the peer cannot choose or refresh that nonce. Pending
approval is tied to that same session, not a client ID supplied by another
connection. Disconnect cancels the request. Approval is durable before the
Authorized response and is independent of camera-start consent.

## Frames

### CLIENT_CHALLENGE — phone → PC, type `0x0a`

| Field | Bytes |
|---|---:|
| Authorization version, exactly 2 | 1 |
| Transport: USB 0, TLS LAN 1 | 1 |
| Phone binding: SHA-256 of TLS leaf DER on LAN, all zero on USB | 32 |
| Random nonce | 32 |

Exactly 66 payload bytes. The PC verifies the transport against its actual
route, and LAN binding against the certificate this connection pinned, before
signing. It never signs for an arbitrary different phone named by the peer.
USB requires an explicit zero binding: there is no cryptographic phone
attestation and no claim to defeat an active physical relay between USB devices.

### CLIENT_AUTH — PC → phone, type `0x0b`

| Field | Bytes |
|---|---:|
| Authorization version, exactly 2 | 1 |
| SEC1 uncompressed P-256 point (`04 || x32 || y32`) | 65 |
| UTF-8 label byte count | 1 |
| UTF-8 label | counted, at most 32 |
| ECDSA P-256/SHA-256 signature, unsigned big-endian `r32 || s32` | 64 |

Payload bounds are 131–163 bytes and are checked from the header before payload
allocation. The count must agree with the actual bytes. Labels use strict UTF-8,
are nonblank and single-line: Unicode controls, format characters and line/
paragraph separators are refused by scalar, not UTF-16 halves. The automatic host label is
printable ASCII or a safe fallback. Names are untrusted presentation, never ID.

The client ID is **derived**, not transferred: the first 16 bytes of SHA-256 over
the exact 65-byte point. Validate the point and any stored key/ID binding before
using a record. An ID match with a different stored point is rejected.

Sign precisely this concatenation:

```text
ASCII("AMB1 PC authorization") || 00 || ASCII("v2") || 00
|| transport_u8 || phone_binding32 || nonce32 || public_point65 || label_byte_count_u8 || label_utf8
```

The point and coordinates/scalars are not little-endian. Android converts raw
signatures to canonical minimal positive DER INTEGERs for its EC provider; Rust
uses the fixed-width ring API. Do not restore raw-width padding in DER.

### CLIENT_AUTH_RESULT — phone → PC, type `0x0c`

Exactly six payload bytes: `status_u8 || reason_u8 || expires_at_unix_u32`.

- Authorized (0): reason None (0), positive phone Unix expiration.
- Pending (1): reason None, expiration 0. Not permission to process media/control.
- Rejected (2): reason Unknown/unapproved (1), Revoked (2), Expired (3), Full (4)
  or Bad signature (5). Only Expired may carry a previous expiration; other
  rejections carry 0. Unrecognized status/reason combinations are malformed.

The phone currently removes revoked rows, so a later proof is unknown/pending,
not a persistent Revoked tombstone. Expired/full/bad proofs are rejected without
an approval request. Renewing a known record is an explicit separate phone UI
operation, not a response to a PC reconnect.

A known valid proof receives Authorized directly. A new valid proof receives
Pending, then one separate Authorized or Rejected result after phone action.
The host polls for Stop while waiting. Repeated Pending, media/capabilities
before Authorized or another proof on the same challenge is a protocol error.

### PAIRING_INFO — phone → PC, type `0x0d` (USB only)

The phone's Wi-Fi pairing details, sent over the USB cable when the user asks for
it, so the first Wi-Fi session needs nothing typed on the PC:

```text
version_u8 (=1) || address_count_u8 (1..=4) || address_count * ipv4_4
|| certificate_sha256_32 || access_key_16
```

- It carries exactly what the PC's three pairing fields ask for: an address, the
  SHA-256 of the phone certificate and the 128-bit access key.
- The PC prefers a private or link-local address when the phone sends several,
  because the other ones are usually tunnels.
- It is **not** an authorization. The PC still proves its key on the Wi-Fi
  session and the phone still approves that PC; this frame only answers "what do
  I type". A phone sends it only when the user presses the button, and only on a
  USB session.
- Trust boundary: the cable is trusted to carry the access key when the user asks
  for that. This is the same limit USB already has — there is no phone attestation
  and no claim to defeat an active relay — and it is why sending is explicit
  rather than automatic.

## Budgets and teardown

The phone independently closes a stalled proof after five seconds and a pending
human decision after 60 seconds. A rejection has at most one second of flush
grace so the existing single writer can deliver its reason; it grants no access
and cannot be extended by traffic. Unknown-request rejection/approval timeout
starts a 30-second prompt cooldown within the current phone owner lifetime.
There is one global physical-session slot across both transports.

Host proof/result processing shares one initial five-second budget. Pending
starts one 60-second human interval, not a per-frame renewable timeout. Partial
AMB1-frame lifetimes and existing transport deadlines remain independently
bounded. Neither side repeats a proof or silently rotates identity on failure;
USB accessory rediscovery is not a new proof or a grant.

Live revoke/expiry closes physical I/O and fences further capture/control. Work
already submitted to I/O cannot be recalled. Phone expiration uses its wall
clock; the phone and its clock are trusted, unlike the PC-proposed name. This is
not hardware/process attestation, a PAKE, an ad-hoc short-code protocol or a
proof against an authorized PC proxying its own access. Binding LAN proofs to
the verified phone certificate prevents a different pinned phone from using the
PC as a signing oracle for a borrowed challenge; USB lacks that phone identity
boundary and makes no corresponding anti-relay claim.

## Cross-language checks

The canonical [frame fixture](fixtures/client-auth-v2.bin) and
[transcript fixture](fixtures/client-auth-transcript-v2.bin) contain public test
data. Both JVM and Rust decode/re-encode the frames and compare transcript bytes.
The crypto primitive has separate shared raw/DER fixtures, including a real
signature with a leading-zero scalar. See [the authorization design and evidence
boundary](../docs/per-pc-authorization.md) before interpreting test results as
end-to-end acceptance.
