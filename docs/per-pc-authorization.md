# Per-PC authorization and revocation

**Unreviewed design with isolated scaffolding, not an enforced security feature.**
Current Wi-Fi access still uses the shared bearer key described in
[security.md](security.md); USB has no per-PC authorization. Neither transport
uses the new exchange in its connection path. The earlier independent transport
review does not cover this proposal. Do not merge or release it on CI results
alone.

## Why separate pairing from authorization?

A bearer key has no owner: losing one copy requires rotating every PC's access.
The intended replacement is a phone-owned public-key list. Bootstrap reaches an
approval request; only an explicit phone action grants camera access. A label
helps a person recognize a request but is attacker-chosen, not an identity.

The intended policy is the same for USB and Wi-Fi:

- An unapproved, expired or revoked client receives neither video nor capture
  control privileges. Expiry must also be checked during a live session.
- Revoking the active client closes its session and stops capture. Renewing
  authorization requires another explicit phone action, never just a reconnect.
- The phone owns approval and first camera start; the PC reports pending,
  rejection and expiry without automatically retrying.
- A bounded list and one bounded pending request prevent unbounded storage and
  prompt spam. Rejected requests need a rate limit/cooldown.

These are requirements, **not current transport behavior**.

## What exists, and what remains

[Host identity](../host-rs/src/identity.rs) generates a P-256 key and derives its
ID from the public point. Its load/create API accepts callbacks, not an OS
store. Only a missing value creates a key; corruption or read failure must not
silently rotate an existing identity. Atomic private-file storage on Linux,
DPAPI storage on Windows and explicit recovery UX remain to be implemented.
Linux file permissions would be access control, not encryption at rest.

[Phone authorization](../android/app/src/main/java/dev/nekomario/amb/ClientAuthorization.kt)
is an in-memory decision model. Its expiry/cap defaults live in that model; it
has no persistent store, approval UI or ownership of a live socket. Removing an
entry currently makes the next valid proof pending again, not permanently
blocked. Re-enrollment/cooldown policy still needs an explicit decision.

[Host exchange](../host-rs/src/client_auth.rs) has a real-TLS synthetic-peer
regression but is not called by connection startup. The prototype codecs and
shared binary fixtures are described in
[the wire document](../protocol/quality-control-v0.md). They are not yet a
complete enrollment protocol:

- `CLIENT_AUTH` does not carry a public key, so an unknown PC cannot be verified
  until a reviewed enrollment key-transfer step exists.
- The prototype signs `clientId || nonce || label`. The phone model accepts a
  caller-built message; it does not own a fresh challenge or bind it to the
  supplied fields, connection or transport. The final protocol needs a
  domain-separated, session-bound transcript and one-shot nonce ownership.
- Direction-specific pre-allocation limits, strict UTF-8 and status/reason
  consistency checks still need to cover the new auth frames.
- Capability negotiation, frame ordering and migration behavior are unresolved.
  Do not use a timeout as feature detection or allow an old client to bypass
  authorization just because the phone's list is empty.

The phone list must eventually persist the **public key as well as its derived
ID**, label and approval/expiry times. Use the Android Keystore protection
boundary, validate the decoded list before use, and expose storage failures
without silently approving a connection. Persistence, approval, camera gating
and live revocation must be tested together before enabling the exchange.

### Signature encoding boundary

The wire carries fixed-width unsigned `r || s`; the platform verifier expects
DER INTEGERs. DER removes redundant leading zeros and adds one sign zero only
when needed. Keeping the raw width produces nonminimal DER and can reject valid
signatures depending on the provider.

The shared fixtures under `protocol/fixtures/client-signature*` contain public
points, messages and signatures only. One deliberately has a leading-zero
scalar. Rust verifies the raw bytes, JVM tests check minimal DER boundary
vectors, and Android instrumentation verifies the same fixtures through the
platform EC provider. Emulator results cover that provider, not every physical
Android device or end-to-end authorization.

## Pairing UX is still a separate security decision

Keep the existing out-of-band certificate pin and TLS access key until a
replacement bootstrap has been reviewed. A locally invented short hash code is
not a reviewed SAS/PAKE protocol: a fresh server nonce alone does not prevent an
active attacker from grinding candidate keys, and it does not solve initial
server authentication. Do not remove the pin/key on that basis.

A QR may transfer an address, pin and short-lived single-use enrollment token
instead of the standing bearer key. This is a proposal, not implemented behavior.
An opaque binary QR can still be extracted or logged by any reader. Expiry and
single use limit exposure but do not stop a stolen token from winning a race;
phone approval and an authenticated binding to the intended PC are still needed.
`FLAG_SECURE` is not protection against an external camera.

## USB needs no QR

Physical attachment supplies the bootstrap path, not cryptographic identity or
standing authorization. Manufacturer/model/serial strings are PC-controlled and
must be presented as claimed metadata, not trust anchors. Matching descriptor
bytes is not a safe reason to inherit another session's camera-start consent.

The USB concerns previously labeled H1–H4 remain acceptance work: camera consent
across replacement sessions, misleading descriptor labels, missing per-PC
revocation, and capture commands from an unidentified peer. Source inspection
alone does not prove Android permission reuse or an automatic-camera-start
exploit. The existing `openAccessory` lifecycle change needs a regression across
all disconnect/replacement paths before claiming H1 fixed.

## Verification boundary

This design does not attest PC software, protect a compromised endpoint or
prevent a person approving the wrong request. Keep prompt/label spoofing,
stolen bootstrap credentials and USB intermediaries in the threat model.

Before enabling it, verify wrong key/ID, changed transcript, nonce replay,
expiry during streaming, explicit renewal, rejected re-enrollment, list/pending
limits, corrupt/failed storage, live revocation and downgrade handling over both
transports. Retain the physical-device and Linux/Windows native-consumer gates
in [security.md](security.md); isolated crypto tests do not satisfy them.
