# Security coverage: what is reviewed, what is tested, what nobody has read

This is **not a review**. It is a map of the security work so that the next
independent assessment (issue #9) starts from what exists instead of from zero, and
so that the parts nothing has looked at are named rather than assumed.

Written 2026-10-21 by the implementer, who cannot be independent of it.

## The three reviews that exist

| Review | Scope | What it could not see |
|---|---|---|
| `secure-wifi-static-review.md` (2026-10-14, a fresh session on `gpt-6.1-sol`, static source only) | Android identity, LAN server, wire, Activity; Rust pin verifier, socket transport, wire | Desktop credential storage, connection selection, manifest/backup rules, outbound queues, lockfiles |
| `per-pc-authorization-review.md` (2026-10-16, independent) | Per-PC authorization: capture teardown, persistence under the shared lock | — |
| `per-pc-authorization-repair-review.md` (2026-10-16, independent) | The repair of AUTHREV-001/002 | Device behaviour for REV-002/REV-004 (marked needs-device) |

All three say what they are not: no penetration test, no physical-device acceptance,
no human sign-off.

## Security claims and the thing that proves each

| Claim | Evidence |
|---|---|
| Wi-Fi is TLS 1.3 with a pinned phone certificate | `host-rs/tests/secure_lan.rs`, emulator negative-path matrix (wrong pin, wrong key) |
| Plaintext never happens after a handshake failure | `secure.rs` has no fallback path; asserted by `secure_lan.rs` |
| A trickled handshake cannot spend an unbounded budget | `secure_lan.rs` (measured: 721 ms against a 200 ms budget in the first attempt, which failed) |
| The access key is compared in constant time | `SecureLanIdentity.authenticates` uses `MessageDigest.isEqual` |
| A PC must be approved on the phone, once, for 90 days | Android unit tests, emulator conformance run, real phone 2026-10-21 |
| Revocation, expiry, too-many and bad-signature refusals are distinct | `client_auth` tests + the refusal table in `gui.rs`, and an AccessKit test that renders it |
| Every decoder a peer reaches before authenticating survives hostile bytes | `tests/parsers.rs` (property, every PR) + `fuzz/` (19.6 M executions, no crashes) |
| A preview record cannot be read by another user | `preview::posio::private_create` (create_new, 0600) |
| Settings that hold the key cannot be world-readable | `Store::load` refuses a file other users can read (measured on a real run) |

## Surfaces added since those reviews, and their trust assumptions

None of these has been read by a reviewer. Each is small, and each has a stated
assumption that a reviewer should either accept or break:

1. **USB pairing hand-off** (`PAIRING_INFO` over AOA). The PC accepts the phone's
   address, certificate fingerprint and access key from the cable, then shows what it
   received and waits for **Connect**. Assumption: *the cable is the trust root*. A
   device on the other end can present its own certificate and key and be trusted,
   because nothing on USB attests the phone (the authorisation path says so
   explicitly). If that assumption is wrong for a threat model, this is the place.
2. **The preview record** (a file in the temp directory). Frames the camera produced,
   `0600`, `create_new`, removed when the window closes. A crash leaves the last
   record behind until the temp directory is cleaned. Assumption: *same-user access to
   the machine is not an adversary*. A reviewer may want the file in
   `$XDG_RUNTIME_DIR` instead.
3. **The PipeWire camera node** (optional feature). Session-managed, so any client in
   the user's PipeWire session can consume it — that is what a camera node is. No
   per-application consent beyond PipeWire's own. Assumption: *PipeWire session access
   equals the user*. Worth confirming against the desktop's portal policy.
4. **Refusal-reason surface**. The PC now displays why the phone refused it. The
   reason is not secret, but it is attacker-influenced text: it goes through the same
   escaping as every other label (no format string, no markup), and the review should
   confirm nothing echoes it into a shell or a log line with secrets.
5. **Settings file location**. `~/.config/nekomario28/Mobile Webcam.conf` is shared
   with the classic Qt application, which the development build overwrites (measured).
   Not a confidentiality issue — the file is `0600` — but a reviewer of the *product*
   should know that two generations of the app write one file.

## What has not been looked at by anyone

- Desktop credential storage on Windows (DPAPI path) and the Android Keystore
  migration story.
- Connection selection (USB device choice) and the inventory thread.
- Android manifest, backup rules, exported components, and the foreground-service
  notification path.
- The outbound queues (`FramedOutbound`, the bounded control channel) under load and
  under a hostile peer.
- Lockfiles and the dependency policy itself: `cargo audit` runs, but nobody has
  reviewed what the dependencies *do*.
- The PipeWire and preview code above, and the refusal text path.
- Nothing here has been tested with an adversary holding the phone, the cable or the
  network — only with tests written by the same hand as the code.

## What an independent reviewer should be given

The three review documents, `protocol/wire-secure-lan.md` and
`protocol/client-authorization-v2.md`, this map, and the sources the first review did
not receive. The fastest way to make that review cheap is to hand it the table above
and ask it to attack the "Evidence" column: a claim whose test passes for the wrong
reason is the failure mode this repository has already hit twice (a capture gate that
accepted a deleted fixture, and a visibility rule that a second writer overruled).
