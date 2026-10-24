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

**2026-10-24 review:** a second independent session on the same model, pointed at what changed
since the first (the `HOST_HELLO` frame, the single-reader handover, the rate limit and pacing,
the new CLI surface, stream alignment). It found three defects, all Low, all in the *fixes* the
first review prompted — see
[independent-review-2026-10-24.md](independent-review-2026-10-24.md), whose disposition also
carries the repair plan. The pattern it names is the one worth keeping: a test that drives a
method rather than the wire is a test that can pin a behaviour no code path reaches. The two
halves it left at that level — ID-001's *routing* and ID-003's *send* — are now driven through a
real socket by `HostHelloAndPairingGateWireTest`, and its WR2/WR3 note — two tests
that would pass for the wrong reason — is closed the same way: those two rules got an owner
(`ReaderHandover`, `StopAction`) and are driven in their own test files.

**2026-10-23 review:** an independent session on `aiand/deepseek-v4.1-flash`, under the
request in `request-2026-10-21.md`, read the areas below and both findings it returned are
fixed — see the disposition at the end of that report. It also named four tests that were
passing for the wrong reason, all four now behavioural, and one of the replacements found
a further real gap (an unrecognised refusal code rendered as generic advice). An
independent assessment is still owed: that session is an outsider to the code but not to
this machine, it ran static source review plus `cargo test`, and it did not touch a
device, the emulator, Gradle, the GUI or the CI workflows.

| Claim | Evidence |
|---|---|
| Wi-Fi is TLS 1.3 with a pinned phone certificate | `host-rs/tests/secure_lan.rs`, emulator negative-path matrix (wrong pin, wrong key) |
| Plaintext never happens after a handshake failure | `secure.rs` has no fallback path; asserted by `secure_lan.rs` |
| A trickled handshake cannot spend an unbounded budget | `secure_lan.rs::trickled_tls_handshake_cannot_extend_connect_deadline` asserts `elapsed < 500 ms` against a 200 ms connect budget, and a sibling does the same for the record-receive deadline. The 721 ms the earlier note cited is a **different claim** — the pre-fix attempt that failed — and the two should not be read as one |
| The access key is compared in constant time | `SecureLanIdentity.authenticates` uses `MessageDigest.isEqual` |
| A PC must be approved on the phone, once, for 90 days | Android unit tests, emulator conformance run, real phone 2026-10-21 |
| Revocation, expiry, too-many and bad-signature refusals are distinct | `client_auth` tests + the refusal table in `gui.rs`, and an AccessKit test that renders it |
| Every decoder a peer reaches before authenticating survives hostile bytes | `tests/parsers.rs` (property, every PR) + `fuzz/` (19.6 M executions, no crashes). This is a *panic* claim, not a memory bound: `Header::validate_payload_size` falls through to `_ => true`, so an unrecognised type may allocate up to `MAX_PAYLOAD` (16 MiB) before any caller can ignore it — bounded at 16 MiB per frame, with a 5 s lifetime |
| A preview record cannot be read by another user | `preview::posio::private_create` (create_new, 0600) |
| Settings that hold the key cannot be world-readable | `Store::load` refuses a file other users can read (measured on a real run) |
| `HOST_HELLO` cannot authorize anything, and ends a session that has already challenged | It carries no identity at all (`readHostFrame` bounds its payload to 0 bytes) and grants nothing: `PhoneAuthorization.onHostHello` ends a session that has already sent its challenge and ignores one that has not. The wire path is pinned by `HostHelloAndPairingGateWireTest.a_host_hello_on_the_wire_ends_the_session_instead_of_being_answered`, proven able to fail against the naive repair that routes the hello to the control handler and re-sends the challenge |
| A peer cannot spin the control channel: PING, `HOST_HELLO` and IDR requests are each limited per **anchored one-second window** | `SessionControl.dispatch` — 64 PINGs, 8 hellos and 8 IDR requests. An overrun is an *error* that ends the session for PING and a *drop* inside the window for the other two. The IDR limit exists because the asymmetry was the finding: an authorized PC could otherwise reach `MediaCodec.setParameters(REQUEST_SYNC_FRAME)` as fast as the session accepts messages. Read, not measured on a device; and the window is anchored, so 64 at the end of one window plus 64 at the start of the next is admitted — this row does not claim a token bucket |
| Two accessory readers never share one endpoint | `AccessorySession.closeAndAwait` keeps the reader handle when the reader survived, `readerAlive()` answers whether it is still running, and `ReaderHandover` holds that *object's* liveness question — durable across attempts and asked now rather than snapshotted (`ReaderHandoverTest`). `MainActivity` refuses the next `openAccessory` on it, pinned by `SessionServiceWiringTest.the_handover_rule_is_the_activity_s_rule`. **Measured on a real endpoint 2026-10-25** ([evidence](../evidence/2026-10-25-usb-reader-release.md)): with the blocked precondition proven on every iteration, a blocked read returned in 0 ms when the descriptor closed, 10 of 10 for a raw read and 10 of 10 for a real session — and again, identically, after reinstalling the branch build itself — and two `openAccessory` calls both succeed, so the platform does not prevent two readers and this guard is the only thing that does. **And the host's own frame, measured 2026-10-25** ([evidence](../evidence/2026-10-25-usb-frame-owner.md)): with one reader blocked on the endpoint the host's 24-byte `HOST_HELLO` completed and that reader's session ended with `PC authorization proof rejected` — the frame reached the reader that *held* the endpoint and was refused at that session's proof gate, which is the ID-001 path on the wire. The control is in the same run: with the endpoint idle the same instrument's iterations end by their own absolute 5 s proof deadline and never with that reason |
| The USB probe cannot measure its own violation | `--usb-probe` paces at 32/s by default, half the phone's PING limit, because an earlier run killed a healthy link: measured *576 round trips across nine seams, 0 mismatches, 0 timeouts* with the probe's own rate as the fault, and `5 FAIL / 60 PASS / 1000 FAIL` before the pacing. After the fix G0.5 passes at 1000 round trips (1000 sent, 1000 echoed, 0 sequence or payload mismatch, 0 timeouts, rtt p50 550 µs, paced 32/s) — [evidence](../evidence/2026-10-24-usb-g0.md). The probe also refuses to PING an unauthorized session |

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

6. **The `HOST_HELLO` frame.** A host holding no session can make the phone end the
   exchange it is in the middle of, which is what a reconnecting host needs because AOA
   reports no disconnect. Assumption: *ending an unauthenticated exchange is not a cost
   worth defending* — the sender gets no identity, no grant and no output, and the rate
   limit above bounds the churn. A reviewer may disagree for a threat model in which the
   peer's only goal is to disconnect the user.

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
- The stop action's credential is a rule with a behaviour test now
  (`StopAction`, `StopActionTest`), but no test presents a real `Intent`: a forged intent and
  a replayed one are exercised at the decision, not through the framework or across processes.
- Nothing here has been tested with an adversary holding the phone, the cable or the
  network — only with tests written by the same hand as the code.

The request that turns this list into work is
[request-2026-10-21.md](request-2026-10-21.md): four areas with file lists, the facts
the implementer read (labelled as pre-work to distrust), and one testable hypothesis
per area.

## What an independent reviewer should be given

The three review documents, `protocol/wire-secure-lan.md` and
`protocol/client-authorization-v2.md`, this map, and the sources the first review did
not receive. The fastest way to make that review cheap is to hand it the table above
and ask it to attack the "Evidence" column: a claim whose test passes for the wrong
reason is the failure mode this repository has already hit twice (a capture gate that
accepted a deleted fixture, and a visibility rule that a second writer overruled).
