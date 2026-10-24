# Independent review (aiand deepseek-v4.1-flash), 2026-10-24

## Scope and evidence boundary

Reviewed the tree at `348e5f0` (clean worktree, VERSION 0.2.0). Read, statically and in
full unless noted: `docs/reviews/{request-2026-10-21,independent-review-2026-10-23,
security-coverage}.md`, `docs/evidence/2026-10-24-usb-g0.md`,
`protocol/client-authorization-v2.md`; Android
`{Wire,SessionControl,AuthorizedMediaSession,AccessorySession,PhoneAuthorization,
ClientAuthorization,ClientRepository,ClientStore,StorageGeneration,SecureLanIdentity,
FramedOutbound,LanServer,SessionService,ClientApprovalUi,PhoneQualityController,
CaptureCancellation,CameraEncoderSession,MediaSession}.kt`, `MainActivity.kt`,
`AndroidManifest.xml`, `res/xml/accessory_filter.xml`, and the JVM tests
(`PhoneAuthorizationTest, QualitySessionTest, SessionServiceWiringTest, WireAndQueueTest`,
the rest listed by grep); host-rs
`{wire,transport,transport/framing,usb,usb_probe,usb_main,client_auth,secure,session,
identity,identity/store,settings,settings/dpapi,settings/registry,preview,preview/posio,
headless,cli,main,video,output/windows}.rs`; `host-rs/Cargo.toml`; the udev rules and
`linux/install-host-integration.sh`; `git log/show` since 2026-10-23.

Ran on this machine:
- `cargo test --manifest-path host-rs/Cargo.toml --lib` → **152 passed, 5 ignored**.
- `--test secure_lan` → 5 passed; `--test parsers` → 2 passed.
- `--lib a_record_replaced` → 1 passed (`preview::tests::a_record_replaced_by_a_symlink_is_refused`,
  one of the 152).
- A JVM harness over the already-compiled `android/app/build/tmp/kotlin-classes/debug`
  classes (jshell, stdin only; no product file touched) that subclasses
  `AuthorizedMediaSession`, calls the real `readMedia(InputStream)` with real AMB1 bytes,
  and drives `PhoneAuthorization` — this is what makes ID-001 and ID-003 *measured* rather
  than argued. Output is quoted in the findings.

Not run, and therefore not claimed: Gradle / the JVM suite (I read the tests and the
recorded results of 2026-10-24 23:32 instead — all 21 methods of `PhoneAuthorizationTest`
green, including both hello tests), the emulator conformance run, any real device, the GUI
capture gate, any CI workflow, a Windows host, the PipeWire backend, the fuzz campaign,
the `io-budget` / `ffmpeg-sys-next` / `pipewire-vircam` internals beyond what the code
calls, the legacy C++ `host/` tree, `windows/` packaging, and `dist/`.

Everything below is new relative to the 2026-10-23 review unless it says otherwise.

## Findings

### ID-001 — a `HOST_HELLO` while the phone waits for a proof *destroys* the session; the documented re-send branch is unreachable from the wire (Low; static for the routing, measured for the effect)

- Where: `android/.../AuthorizedMediaSession.kt:61-65` (routing), `SessionControl.kt:38-47`
  (rate limit and callback), `PhoneAuthorization.kt:166-176` (`onHostHello`),
  `PhoneAuthorization.kt:172` (renewal), `PhoneAuthorization.kt:180-184`
  (`receive`'s type requirement), `PhoneAuthorizationTest.kt:186-200`,
  `protocol/client-authorization-v2.md:21-25`.
- Sequence: the phone is in `PROOF` (it has sent its one challenge). A host that
  reconnected sends `0x0e`. `readMedia` asks `gate.isAuthorized()`, which is false in
  `PROOF`, so the frame goes to `gate.receive(...)` — not to `control.dispatch(...)`.
  `receive` does `require(header.type == Wire.TYPE_CLIENT_AUTH)`, which fails, is caught,
  and calls `finish("PC authorization proof rejected")`. `Session.onHostHello()` is never
  reached, so its re-send branch never runs.
- Observed vs argued: **observed**. Driving the real `readMedia` with a real `0x0e` frame
  against a session that had just sent its challenge (90 bytes out):

  ```
  after start: authorized=false bytes-sent=90
  === feeding HOST_HELLO (0x0e) now, exactly as the reader does ===
  threw: java.lang.IllegalArgumentException: PC authorization required before controls
  closed(reason)=[PC authorization proof rejected]
  authorized after=false
  ```

  The protocol document's first bullet ("waiting for a proof — re-send the *same*
  challenge") and the CHANGELOG/commit message that introduced the frame both describe a
  behaviour that no wire path can reach: `control.dispatch` is reached only while
  authorized, so the *only* phase in which the wire delivers a hello to the handler is the
  one where it ends the session. The document's third bullet (the backwards-compatibility
  story — "an app that does not handle the type treats it as an unexpected control frame,
  ends that session and rediscovers") is an accurate description of v2 as built.
- Second consequence, latent: the PROOF branch is the one place that **renews** the proof
  deadline (`deadline = nowMillis() + PROOF_TIMEOUT_MS`, line 172), while `tickLocked`
  (lines 258-265) states the opposite rule in the same file ("Absolute and non-renewable on
  purpose") and `stalled_proofs_and_pending_approvals_have_absolute_nonrenewable_deadlines`
  pins it. The obvious repair for the routing defect — dispatching `0x0e` to the gate before
  the authorized check — would therefore introduce a window an unauthenticated adjacent peer
  can hold open indefinitely, and make the phone emit a challenge on every hello up to
  `MAX_HELLO_PER_SECOND` (8/s). Today that is dead code, not an exposure; the repair must
  drop the renewal and bound the re-sends at the same time.
- Impact boundary: ending an authorized session on hello is not a new capability — the same
  peer can close the socket, and on USB the cable is the declared trust root — so no
  privilege change. What is wrong is the divergence: a documented recovery path that does
  not exist, a test that pins it, and a latent violation of a pinned invariant.
- Evidence boundary: routing is static (`AuthorizedMediaSession.kt:61-65`); the session-
  ending effect is measured on compiled app classes, no device. The 8/s cap and the PING
  fixed window I could only read.

### ID-002 — the "never two readers on one endpoint" guard is defeated by the path that enforces it (Low; static)

- Where: `MainActivity.kt:1082` (`?: true`), `:729-736` (refuse, then reconnect),
  `:713-718` (`scheduleRediscover`), `:663-665` (`discoverUsb` requires `session == null`),
  `AccessorySession.kt:68-79`, `SessionServiceWiringTest.kt:160-184`.
- Sequence: (1) a session's reader does not stop inside the 1500 ms join, so
  `closeAndAwait` returns false and `closeSession` returns false. (2) `openAccessory` refuses
  and calls `scheduleRediscover()`. (3) `closeSession` had already set `session = null`, so
  the rediscovery one second later passes `discoverUsb`'s `session != null` guard and
  re-enters `openAccessory`. (4) There, `closeSession()` evaluates
  `(current as? AccessorySession)?.closeAndAwait(...) ?: true` with `current == null` and
  returns **true** unconditionally, so the new `AccessorySession` is created and started
  next to the surviving reader. The guard holds for exactly one iteration, and its own
  refusal instruction ("Unplug and replug the cable") is contradicted by the retry.
  Independently, `closeAndAwait` sets `readerThread = null` *before* it answers, so a second
  call on the same session always reports success — the answer is not durable.
- Precondition, not hypothetical: the code's own comment says the failed stop was measured
  on a real phone (2026-10-24), producing `BAD_SIGNATURE` and "expected authorization
  outcome before capabilities/media".
- Observed vs argued: the bypass is **static** (the four code points above). The consequence
  (two readers on one endpoint) is argued from the app's own measurement, not reproduced
  here. I could not turn it into an authorization change: `closeMedia` closes the gate and
  frees the single owner slot, so the second session authorizes normally; the damage is the
  frame-stealing/protocol-confusion the guard was written for, plus a rediscovery loop.
- Evidence boundary: static. Needs a device to observe the surviving reader.

### ID-003 — the Wi-Fi pairing hand-off is the only media-session sender with no authorization gate, and the UI offers it exactly while unauthorized (Low; measured for the send, static for the rest)

- Where: `AuthorizedMediaSession.kt:101` versus `:86-104` (every other sender checks
  `permitted()`), `MainActivity.kt:1024` (visibility uses `session != null`), `:259-282`
  (the handler), `client_auth.rs:104,167` (the host refuses a non-outcome frame).
- Sequence: a USB session is attached but not yet authorized (`PROOF`/`PENDING`), which is
  precisely when `sendPairingAction.visibility = ... && session != null` is true. The user
  opens Details and presses it; `sendPairingInfo` has no `permitted()` check and writes
  `0x0d` immediately. The host, still inside `client_auth::exchange`, sees a frame that is
  not `CLIENT_AUTH_RESULT` and errors ("expected authorization outcome before
  capabilities/media", or "does not support required per-PC authorization v2" if it happens
  before the challenge). The session dies; `a_hello_would_help` matches both strings, so the
  host then hellos and retries — its own recovery masks it.
- Observed vs argued: **observed** (the ungated write), from a session in `PROOF`:

  ```
  authorized=false (phase PROOF), session bytes=90
  PAIRING_INFO written while unauthorized: 76 bytes (header+52 = 76 expected)
  frame type byte at offset 95 = 0x0d
  ```

- Impact boundary: the design's stated trust boundary (the cable, and an explicit user
  press) is respected, so the access key does not reach a peer the protocol excludes. But
  the frame carries the 128-bit Wi-Fi access key, and the only thing gating it is a UI
  visibility test — the API has none. Any future caller (an automatic send, a second
  surface) inherits that. The frame also breaks the exchange it is sent on.
- Evidence boundary: measured for the ungated send; static for the UI condition and the host
  refusal. A device run would confirm the user-visible sequence.

## Claims I could not falsify

- **"The hello carries no identity and grants nothing."** Attempt: fed `0x0e` to a
  `PROOF`-phase session and to the compiled `SessionControl`; the phase never became
  `AUTHORIZED`, no `CLIENT_AUTH_RESULT` was written, and no storage write occurred. It ends
  sessions and (on the unreachable branch) would re-send a challenge. Holds.
- **"The phone sends no capabilities/media and accepts no quality/PING/IDR controls until
  Authorized."** Attempt: `readMedia` routes *every* pre-authorization frame to the gate;
  measured that `0x0e` is rejected there, and `camera_controls_cannot_be_used_as_an_
  authorization_bypass` covers PING/IDR/PREPARE/COMMIT. No bypass found. (Note the same
  routing is ID-001.)
- **"Every decoder a peer reaches before authenticating survives hostile bytes."** Ran
  `--test parsers` (2 passed) and read `wire.rs`. Holds for panics; the cell should not be
  read as a memory bound: `Header::validate_payload_size` falls through to `_ => true`, so
  an unknown type with `payload_len` up to `MAX_PAYLOAD` (16 MiB) is allocated by the shared
  reader before any caller can ignore it. Bounded at 16 MiB per frame, and one frame has a
  5 s lifetime.
- **The preview Q1 fix (`private_reopen`).** Read `preview/posio.rs:51-79` and ran the
  behavioural test `a_record_replaced_by_a_symlink_is_refused` (green): `O_NOFOLLOW` plus a
  regular-file / owner / mode re-check, and the test asserts both refusal and that the link
  target was not written. Genuinely closed. Residual: a same-uid *hard link* to that user's
  own file passes the checks — inside the file's stated "same-user is not an adversary" line.
- **"A peer cannot extend its own proof window."** Holds as implemented, and only because
  ID-001's renewal path is unreachable. The pinned test does not cover the one code path
  that renews.
- **The USB reader cannot be steered to hide or duplicate a frame (`align_stream`, the
  `pending`/`scratch` path).** Attempt: traced `usb.rs:335-363` and `Transport for Usb`
  (`usb.rs:382-430`); `pending` is drained exactly once and never re-read, every header is
  decoded and type-bounded before allocation, and `read_bulk` fills a 1 MiB scratch so a
  coalesced transfer cannot desynchronise the reader. I could not construct a dropped,
  duplicated or foreign-session frame. Two caveats, below as Q1/Q2.
- **The phone's PING window arithmetic.** Read `SessionControl.kt:28-35`. It is a fixed
  window anchored at the first PING after the previous window closed, so 64 at the end of
  one window plus 64 at the start of the next (`now - pingWindowStart >= 1s` resets the
  count) is admitted — up to ~128 in a burst. Not a falsification of "64 per second" as
  written; the coverage-table wording should say "per anchored one-second window".
- **The coverage table's trickle row.** Its cited measurement ("721 ms against a 200 ms
  budget in the first attempt, which failed") is a 3.6× overrun, while the guard that
  replaced it asserts only `elapsed < 500 ms` (2.5×). The row is honest about the pre-fix
  attempt; the number and the guard are different claims and should not be read as one.
- **The new CLI surface (priority 4).** Read `cli.rs`, `headless.rs`, `main.rs`. Every
  option is validated (`--frames/--rate/--budget-ms/--seconds` positive, `--transport
  usb|lan`, `--quality` non-zero and strictly `WxH@fps`), the diagnostics
  (`--usb-dump`, `--usb-frames`, `--usb-probe`) write and delete nothing, and
  `--replace-identity` is the only destructive path: it is refused unless the stored key is
  unreadable (`stored_is_readable`, which does not create one), needs `--force` otherwise,
  and *renames* the old key. `--headless --host` overrides the address but not the pinned
  fingerprint, and `SecureTcp::connect` sends the access key only after
  `verify_server_cert` has matched the pin, so pointing it at another host does not leak the
  key. None of it is reachable by a peer; all of it is local CLI.

## Tests that would pass for the wrong reason

1. **`a_host_hello_resends_the_same_challenge_while_waiting_for_a_proof`**
   (`PhoneAuthorizationTest.kt:186-200`) calls `h.session.onHostHello()` directly. Nothing
   in the suite sends `0x0e` through `SessionControl`/`readMedia` into the gate, so the test
   stays green while the wire path ends the session (ID-001). Its sibling
   `a_host_hello_cannot_be_spun` is in the same position — its comment even says "after it,
   PROOF re-sends — that is the test above" — and it passes because the session is already
   closed by then, not because a hello cannot be spun on the wire.
2. **`a_new_usb_session_is_refused_while_the_previous_reader_is_alive`**
   (`SessionServiceWiringTest.kt:160-184`) is three substring assertions on source text
   (`"if (!closeSession())"`, `"closeAndAwait("` + `"return stopped"`,
   `"fun closeAndAwait(...): Boolean"` + `"return !reader.isAlive"`). All three are present
   and the behaviour is still absent (ID-002), because the assertions cannot see `?: true`,
   the rediscovery, or that the refusal path re-enters the function it is guarding.
3. **`the_stop_action_is_not_a_credential_any_app_can_present`**
   (`SessionServiceWiringTest.kt:204-219`) asserts `activity.contains("UUID.randomUUID()
   .toString()")` over the **whole file**. `MainActivity.kt:687` already uses
   `UUID.randomUUID().toString()` for `usbPermissionRequestId`, so `stopTokenValue` could be
   reverted to a constant and this test would still pass while
   `consumeStopIntent`'s substring assertions still matched. There is no test that feeds a
   forged intent — the 2026-10-23 disposition says the replay was not verified on a device,
   and nothing in the JVM suite replaces it.
4. **`stalled_proofs_and_pending_approvals_have_absolute_nonrenewable_deadlines`** — the
   name overstates what it covers: it advances the clock only, so it never exercises the one
   path that renews the deadline (`onHostHello`'s PROOF branch, ID-001).
5. Recorded so the maintainer knows it was checked, not as a defect:
   `gui::tests::every_reason_code_reaches_a_rendered_title` genuinely drives code →
   `reason_text` → `refusal()` → rendered title, so 2026-10-23's item 2 is closed;
   `WireAndQueueTest.pairing_code_comparison_is_exact` makes the constant-time claim a
   behaviour; and `QualitySessionTest.aCommitAtIdleNeverStartsTheCamera` sends the
   `COMMIT_QUALITY` the Area-C hypothesis needed. **However**, `security-coverage.md` has no
   row at all for `HOST_HELLO`, the single-reader handover, the PING/IDR/hello limits, or
   the probe and its pacing — the four newest behaviours, two of which (ID-001, ID-002) are
   guarded only by the substring tests above.

## Questions

- **Q1 — `align_stream`'s "aligned" verdict is a guess after silence, and `connect`
  discards it.** `usb.rs:342-345` returns `Ok(true)` when a single 200 ms `read_raw`
  times out ("Nothing queued: the next thing this host reads is a fresh header"), and
  `Usb::connect` writes `let _ = opened.align_stream();` (`usb.rs:311`). A phone
  that pauses mid-payload for longer than the read timeout is declared aligned while the
  next byte is a tail. `reset_stream`'s result *is* checked in the probe
  (`usb_probe.rs:404`), so the shipping path is the one that does not look. I could not
  build a repro without a device; the failure mode should be a visible
  `invalid AMB1 header`, not a silently wrong frame.
- **Q2 — the shipping hello retry cannot realign the stream it retries on.**
  `video.rs:207-224` retries the exchange after `reset_exchange` but the comment at
  `video.rs:218-221` records why it cannot call `reset_stream` (the transport is already a
  trait object). `a_hello_would_help` (`usb_probe.rs:326-337`) matches "incomplete frame"
  and "does not support required per-PC authorization v2", which are exactly the
  misalignment symptoms, so the shipping retry fires in the cases where success depends on
  the reader being realigned. Did the retry ever complete on hardware, and was the
  half-read stream ever the reason it did not?
- **Q3 — Windows: the frame-delivery objects are session-global and the producer guard is
  squattable.** `output/windows.rs` opens `UnityCapture_Mutx`, `UnityCapture_Want`,
  `UnityCapture_Sent` and `UnityCapture_Data` without a `Local\`/`Global\` prefix, so which
  process receives the camera frames is decided by whoever created them first in that
  Windows session; and `Camera::open` treats `Local\MobileWebcam_UnityCaptureProducer`
  already existing as "another producer", which another process in the session can arrange.
  Both are within the documented "same-user access is not an adversary" line, and I have no
  Windows host to test either. Worth confirming against the pinned consumer's namespace if
  the model ever excludes co-session processes.
- **Q4 — the remaining trust assumptions in `security-coverage.md` were read, not exercised.**
  The PipeWire node (`output/pipewire.rs`) and the refusal-text rendering path were read and
  found consistent with their stated assumptions, but nothing here started a PipeWire consumer
  or rendered the text on a real window; the pairing hand-off is ID-003 and the preview record
  is the bullet above.


---

## Disposition (implementer, 2026-10-24)

All three findings are accepted and **fixed below** (one commit, in the order the review lists
them). The note that follows records what each change does and what it still does not cover.

**ID-001 — FIXED.** `onHostHello` no longer has the unreachable `PROOF` branch and no longer
renews the proof deadline; the rule is now "a session that has already challenged ends, one that
has not yet challenged ignores the hello", which is what the wire already did (controls are
routed to the gate only while authorized, so a hello during `PROOF` is refused there and ends the
session). `protocol/client-authorization-v2.md` says the same, and the test that pinned the
unreachable re-send was replaced by one that pins the wire's behaviour. **What is still not
covered: the routing itself** — a socket-level test that feeds `0x0e` through a real session and
asserts the end. The session half is pinned; the dispatch is read, not tested.

**ID-002 — FIXED.** `closeAndAwait` keeps the reader handle when the reader survived (clearing it
made a second call report success unconditionally), `readerAlive()` answers whether it is still
running, and `MainActivity` remembers the *object* whose reader refused to stop — refusing the
next `openAccessory` on that object rather than on a field that `closeSession` had already
cleared. The refusal is therefore durable rather than one iteration deep. **Not covered: a
device observation** that two readers really cannot coexist after the refusal.

**ID-003 — FIXED.** `sendPairingInfo` is gated on `permitted()` like every other sender, and the
action is offered only while the session is authorized — which is what the hand-off is for.
**Not covered:** the button's visibility has no test; the write is gated in the session, which is
where the reviewer measured it.

The original note follows, unedited, because it says what the pass did not reach.

### Coverage added after the disposition (2026-10-24, later pass)

The disposition above says, for each fix, what it does **not** cover. Three of those gaps are now
closed, and the change that closes them is the one the review named as the pattern to watch for:
a test that drives the wire instead of a method.

- **ID-001's routing is tested on the wire.**
  `HostHelloAndPairingGateWireTest.a_host_hello_on_the_wire_ends_the_session_instead_of_being_answered`
  writes `0x0e` into a real socket and lets the session's own reader thread call `readMedia`,
  then asserts that the session ends naming the *refused proof* (not an expired deadline) and
  that no challenge is re-sent. **Proven able to fail:** restoring the naive repair this report
  warns about — route the hello to the control handler, re-send the challenge in `PROOF`, renew
  the deadline — reddens exactly this test and leaves the other two in the file green.
- **ID-003's send is tested as a refusal, and so is its button.**
  `the_pairing_handoff_writes_nothing_until_the_pc_is_approved` drives the same real session:
  the hand-off is silent while the PC is unauthorized and delivers the identical payload once
  `approve()` has run — so the gate, not a missing sink, is what suppressed it.
  `SecondarySurfaceWiringTest.the_pairing_handoff_is_offered_only_for_an_authorized_pc` pins
  the button's rule, which had no test at all.
- **Issue #78, item 1** (no control frame before the authorization result) is pinned by
  `no_capabilities_reach_the_wire_before_the_authorization_result`: the advertisement is queued
  while unauthorized and arrives only behind the result.

Removing either authorization gate (`sendPairingInfo`, `updateCapabilities`) fails all three
tests in a mutation run, so none of them is a test that passes for the wrong reason.

Still open, from the review's own list: **WR2**
(`a_new_usb_session_is_refused_while_the_previous_reader_is_alive`) and **WR3**
(`the_stop_action_is_not_a_credential_any_app_can_present`) remain substring assertions on source
text — both need an Activity-level harness this suite does not have — and **WR5**'s gap stands:
`security-coverage.md` still has no row for `HOST_HELLO`, the single-reader handover, the
PING/IDR/hello limits, or the probe and its pacing.

### Original disposition note

All three findings are accepted. They are **not fixed in this commit**: each one is a change to
the phone's session handling, and each needs the lesson of ID-001 applied to its own test (drive
the wire path, not the method). Writing them down precisely is the deliverable of this pass; the
work is queued in that order.

### ID-001 — the documented hello branch is unreachable, and its test bypassed the wire (FIX)

Confirmed by reading the routing: `AuthorizedMediaSession.readMedia` sends a frame to
`control.dispatch` only when `gate.isAuthorized()`, so a hello arriving while the session waits
for a proof is fed to `gate.receive`, which refuses any type other than `CLIENT_AUTH` and ends
the session. `onHostHello`'s PROOF branch — and the deadline renewal inside it — is dead code,
and `a_host_hello_resends_the_same_challenge_while_waiting_for_a_proof` passes only because it
calls the method directly. The reviewer is right that the naive repair (dispatch the hello
before the authorized check) would open a window an adjacent peer can hold open, because that
branch is the one place that renews a deadline the same file pins as absolute.

Planned fix, in one change:

1. `onHostHello`: drop the renewal. The rule becomes "a session that has already sent a
   challenge ends; one that has not yet challenged ignores the hello" — which is what the wire
   does today, so code, doc and wire agree in both directions.
2. `protocol/client-authorization-v2.md`: replace the three bullets with that rule, and say
   plainly that the backwards-compatibility story (an app that does not know the type ends the
   session and rediscovers) is what the current device already does.
3. Tests: delete the method-driving test that pins the unreachable branch, and add one that
   feeds `HOST_HELLO` through the **real** dispatch path (a `SessionControl` over a real
   `FramedOutbound`) in both phases, asserting what the wire actually does.

### ID-002 — the single-reader guard holds for exactly one iteration (FIX)

Also confirmed by reading: `closeSession` sets `session = null` before answering, so the
rediscovery it schedules re-enters `openAccessory` where `(current as? AccessorySession) ?:
true` sees `current == null` and reports success — and `closeAndAwait` clears `readerThread`
before it decides, so a second call on the same session always claims the reader stopped. The
guard was mine (#80) and it does not hold.

Planned fix: keep the answer durable — remember that a reader failed to stop (a field on the
service or the activity, cleared only when a later attempt finds the reader gone), do not clear
`readerThread` until after the join decision, and make the refusal *stick* instead of scheduling
a rediscovery that bypasses it. A test that drives the sequence twice is part of it.

### ID-003 — the pairing hand-off writes while unauthorized (FIX)

Confirmed by measurement in the review, and it is the shape the host's own retry now hides: a
`0x0d` inside `client_auth::exchange` is a non-outcome frame, the host errors, and then its
`HOST_HELLO` recovery makes the symptom look like a session seam. The hand-off exists to move
the Wi-Fi key to a PC that is *approved*, so `permitted()` is the correct gate — the button
should be offered only for an authorized session, with the status line saying to approve first.

Planned fix: add the `permitted()` check to `sendPairingInfo`, hide the action unless the
session is authorized, and test the ungated write as a refusal.

### What this pass did not reach

Windows credential storage, connection selection, manifest/backup rules, outbound queues under
load, the dependency internals, and no device work. The reviewer also lists five tests that
would pass for the wrong reason; those are folded into the three fixes above, and the pattern
they share — a test that drives a method rather than the wire — is the one to watch for in the
next pass.
