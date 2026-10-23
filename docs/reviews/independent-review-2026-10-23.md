# Independent review (aiand deepseek-v4.1-flash), 2026-10-23

## Scope and evidence boundary

Read (static, full files unless noted):
`host-rs/src/{identity.rs, identity/store.rs, settings.rs, settings/dpapi.rs, usb.rs,
transport.rs, transport/framing.rs, secure.rs, client_auth.rs, client_auth/tests.rs,
wire.rs, session.rs, control.rs, video.rs, preview.rs, preview/posio.rs, output.rs,
output/linux.rs, output/pipewire.rs, gui.rs, gui/harness_tests.rs, cli.rs, main.rs,
headless.rs, usb_main.rs, platform.rs}`; `android/.../{MainActivity.kt, SessionService.kt,
FramedOutbound.kt, AuthorizedMediaSession.kt, AccessorySession.kt, SessionControl.kt,
QualitySwitch.kt, PhoneQualityController.kt, CameraEncoderSession.kt, CameraBridgeController.kt,
Wire.kt, ClientAuthorization.kt, PhoneAuthorization.kt, ClientRepository.kt, ClientStore.kt,
SecureLanIdentity.kt, LanServer.kt, StorageGeneration.kt, ClientApprovalUi.kt, MediaSession.kt,
H264Encoder.kt}`; `AndroidManifest.xml`, `res/xml/accessory_filter.xml`; the JVM tests
(`QualitySwitchTest, QualitySessionTest, PhoneAuthorizationTest, AuthorizedMediaSessionTest,
WireAndQueueTest, SecondarySurfaceWiringTest, SessionServiceWiringTest,
CaptureCancellationWiringTest, UsbTransportFramingTest`) and both instrumentation suites;
`host-rs/tests/{secure_lan.rs, parsers.rs, session.rs, quality_switch.rs, common/mod.rs}`;
`host-rs/fuzz/fuzz_targets/wire_decode.rs`; `.github/workflows/secure-wifi-android-emulator.yml`.

Ran (executed, on the review machine):
- `cargo test --manifest-path host-rs/Cargo.toml --lib` → 137 passed, 4 ignored.
- `cargo test --manifest-path host-rs/Cargo.toml --test secure_lan` → 5 passed.
- `cargo test --manifest-path host-rs/Cargo.toml --test parsers` → 2 passed.

Not run: any Android build/Gradle/JVM test, the emulator conformance run, any real
device, the GUI capture gate, the release/Windows jobs, the actual fuzz campaign, the
CI Python TLS client, and the `io-budget` / ffmpeg / PipeWire dependency sources.

Severity below reflects the precondition I could actually observe. "static" means I
read the path and did not execute it.

## Findings

### ID-001 — Any other app on the phone can stop a live capture through the exported Activity (Low; static)
- Where: `android/app/src/main/AndroidManifest.xml:23` (`android:exported="true"`),
  `android/app/src/main/java/dev/nekomario/amb/MainActivity.kt:389-397` (`onResume`
  matches `intent?.action == ACTION_STOP_SESSION`, then `stopCamera()`),
  `MainActivity.kt:1081` (`ACTION_STOP_SESSION` is a plain string constant).
- Sequence: user starts streaming and backgrounds the app (the foreground-service case
  the feature exists for). A second, installed app calls
  `startActivity(Intent().setClassName("dev.nekomario.amb","dev.nekomario.amb.MainActivity")
  .setAction("dev.nekomario.amb.STOP_SESSION").addFlags(FLAG_ACTIVITY_NEW_TASK))`.
  The Activity is exported, the action is unauthenticated and requires no permission,
  and an explicit-component start ignores intent filters. The Activity is brought
  forward, `onResume` runs, matches the action, calls `stopCamera()`, which also
  `SessionService.stop(this)` and cancels the quality controller.
- Observed vs argued: I read the path and the manifest; the only caller that *should*
  produce this action is the notification `PendingIntent` in `SessionService.kt`
  (which is itself safe: same-package identity, `FLAG_IMMUTABLE`). Nothing binds the
  received action to the notification. There is no device run.
- Impact boundary: denial of service of the user's own capture only — no frame
  disclosure and no authorization change; the session/transport stays connected and the
  user can start again. Repro is `adb shell am start -a dev.nekomario.amb.STOP_SESSION
  -n dev.nekomario.amb/.MainActivity` while streaming.
- Evidence boundary: static (call path + manifest). Device observation is cheap but
  was not done here.

### ID-002 — An authorized PC can request IDR frames with no bound (Low; static)
- Where: `android/app/src/main/java/dev/nekomario/amb/SessionControl.kt:32`
  (`TYPE_VIDEO_IDR_REQUEST -> outbound.forceRecovery()`), with no counter next to the
  PING path at `SessionControl.kt:22-28` (`require(++pingCount <= 64)`).
- Sequence: after authorization, the PC sends `VIDEO_IDR_REQUEST` frames as fast as the
  session accepts them. Each one calls `FramedOutbound.forceRecovery()`, which clears
  the video queue and calls `onNeedIdr()` → `CameraEncoderSession.requestIdr()` →
  `H264Encoder.requestIdr()` → `MediaCodec.setParameters(PARAMETER_KEY_REQUEST_SYNC_FRAME)`.
  Every call asks the encoder for a fresh intra frame.
- Observed vs argued: I read the dispatch and the encoder call; PING is explicitly
  rate-limited and this message is not, so the asymmetry is real. I did not measure the
  bandwidth/battery cost.
- Impact boundary: resource consumption by a peer the user already approved
  (precondition: an authorized PC, i.e. the user tapped Allow, or a previously approved
  PC reconnecting). No confidentiality/integrity effect. Rate-limiting this the way
  PING is limited, or coalescing pending IDR requests, would close it.
- Evidence boundary: static.

## Claims I could not falsify

**A — replacing the identity does not resurrect approvals.** Attempt: traced
`Identity::from_der`/`load_or_create` (`identity.rs:74`, `identity/store.rs:19`) into
`receive` (`video.rs:172`) and the phone side `ClientAuthorization.evaluate`
(`ClientAuthorization.kt:96`). The PC id is `SHA256(public_point)[..16]`
(`identity.rs:126`) and is re-derived on the phone, and a stored entry whose point
differs is rejected as `BAD_SIGNATURE`. No host-side approval cache exists. A different
valid key is therefore unknown, not approved. The `replace_stored` quarantine
(`store.rs:52-100`) only moves the old file aside. I could not make a replacement
resurrect a grant.

**A — corrupt/truncated identity fails closed.** `read_at` refuses non-file types,
oversize, wrong owner/mode (Unix) and validates the hex before `from_der`
(`store.rs:104-146`). Tests `reading_tells_missing_corrupt_and_readable_apart` and
`corruption_never_replaces_an_existing_file` exercise this. No half-read state found.

**B — a flapping device cannot steer the session to a different device.** Attempt:
traced `Usb::connect` → `selected(..., false)` (exact `DeviceId` including bus, address,
vid, pid, port chain) → `switch` → `selected(..., true)` which accepts only an
*accessory* on the same `(bus, ports)` physical path (`usb.rs:118-140, 208-232`). A
disconnect before open returns "selected USB device disconnected"; `wait_accessory`
never accepts a non-accessory and times out. The GUI's `selected`-loss handling
(`gui.rs:884-903`) only re-points to an accessory on the same physical path while
running. I found no remote path to a different device.

**B — the inventory queue is bounded.** Attempt: `gui.rs:333-356` uses
`mpsc::sync_channel(1)` and `try_send`, and only republishes when the signature changes.
No unbounded growth.

**C — an approved PC cannot start capture by itself.** Attempt: traced every setter of
`cameraStartRequested` (`MainActivity.kt:197` button, `MainActivity.kt:469` startCapture lambda) and every producer of
`QualitySwitch.Action.StartCapture` (`QualitySwitch.kt:59-68`). `StartCapture` is emitted
only by `Commit` while `phase == Prepared`, and `Prepared` is entered only from
`Prepare(streaming = true)` where `streaming = { cameraBridge != null }` — i.e. capture
was already running by the user's Start. `Commit` in `Staged` is an explicit no-op, and
`startCamera` itself returns unless `cameraStartRequested` is already true
(`MainActivity.kt:761-762`). New physical/Wi-Fi sessions clear the flag via `closeSession` /
`stopCamera`. I could not reach `startCamera()` with the flag false.

**C — an accessory that claims our strings cannot authorize a PC.** The filter opens
the Activity only; the session still runs the v2 challenge/approval gate, and the user
must tap Allow. A replayed proof fails on the per-session random nonce.

**D — a stalled reader does not stall the other direction or grow memory.** Attempt:
on the phone, reader (`AccessorySession`/`LanSession`) and `FramedOutbound` writer are
separate threads; the control queue is capped at 32 and the video queue at
`videoCapacity` (default 2) (`FramedOutbound.kt:24, 33, 168`). On the host, the worker
processes frames synchronously (no receive queue), the parent/worker event channel is
`sync_channel(32)` (`session.rs:180`), and every host→phone frame is length-bounded to
163 bytes (`Wire.readHostFrame`). `WireAndQueueTest.blockedWriterDoesNotAccumulateUnlimitedControlFrames`
and `QualitySessionTest.pausedBarrierDropsQueuedAndLateOldGenerationVideo` pass. The
remaining exposure is the accepted one from the repair review (auth monitor held for
the `outbound.close()` join of ≤500 ms, `AuthorizedMediaSession.kt:73-79`).

**E — the refusal text cannot inject anything.** `Outcome::Rejected` carries
`reason_text(u8)` from a fixed match (`client_auth.rs:71-79`); the GUI maps the string
to a fixed `Refusal` (`gui.rs:635-650`) and renders it as a label. The peer controls only
a `u8` reason code. No shell, format string or markup path found.

## Tests that would pass for the wrong reason

1. **The "constant-time" access-key claim is guarded by a substring assertion, not a
   behaviour.** `SecondarySurfaceWiringTest.a_connection_uses_the_binding_bytes_instead_of_reparsing_the_fingerprint`
   (`android/.../SecondarySurfaceWiringTest.kt:106-148`) is the only guard for
   `SecureLanIdentity.authenticates`; it asserts
   `identity.contains("MessageDigest.isEqual(candidate, secretBytes)")` (line 141). It
   reads source text, so it stays green if the substring survives anywhere (a comment, a
   dead branch, a helper) while the live comparison changes, and it never observes the
   comparison's timing. There is no behavioural test of `authenticates` at all. (I
   believe the claim itself holds — lengths are fixed at 16 and `MessageDigest.isEqual`
   is constant-time for equal-length arrays — but the Evidence column overstates the
   guard.)

2. **`gui/harness_tests.rs:76 a_revoked_pc_is_told_it_was_revoked_not_that_the_key_is_wrong`
   injects the result it claims to prove.** It sets
   `session.state = State::Failed("this PC's access was revoked on the phone")` directly
   and asserts the rendered label. It therefore proves the *renderer*, not the
   wire-reason → `reason_text` → `refusal` chain named in the coverage table; the chain
   is only joined by shared literal strings across two other tests
   (`client_auth/tests.rs:each_rejection_reason_reaches_the_user_as_text`,
   `gui.rs:1587 each_refusal_reason_is_recognised`). If the connection path stopped
   producing that text, this test would stay green.

3. **The Area-C capture hypothesis has no test that attempts it.**
   `ClientAuthorizationUiInstrumentation.checkStoppedCamera`
   (`android/app/src/androidTest/.../ClientAuthorizationUiInstrumentation.kt:200`) only
   asserts the Start button is enabled and labelled "Start camera" after authorization.
   No test sends `PREPARE_QUALITY` (and in particular none sends `COMMIT_QUALITY`) to a
   non-streaming authorized session and then asserts that no `startCapture` happened.
   `QualitySessionTest.deniedPermissionAndIdleStagingNeverStartTheCameraFromPc`
   (`QualitySessionTest.kt:74`) sends a `Prepare` only; the commit-at-idle path that the
   hypothesis is about is never exercised through the real controller. The claim is
   consistent with the code I read, but the named evidence does not test it.

4. **The preview symlink claim is asserted in a comment, and the tests exercise the
   unprotected fallback.** `preview/posio.rs:18` says `create_new` refuses a pre-planted
   symlink, but `Producer::open` (`preview.rs:93-100`) takes the `AlreadyExists` branch —
   which is the *normal* path, because the GUI calls `create_path()` first
   (`gui.rs:358`) — and re-opens with plain
   `OpenOptions::new().read(true).write(true).open(path)` (no `O_NOFOLLOW`, no owner/mode
   re-check). The preview tests build the record exactly that way, so they exercise the
   fallback but never assert it refuses a symlink. The `create_new` protection only
   covers pre-planting, which is what the comment describes; the substitution window is
   untested.

## Questions

- **Q1 — preview record writes can follow a substituted path.** If an attacker can
  replace the GUI-created `mobile-webcam-preview-<pid>-<nanos>.bin` between
  `create_path()` and the worker's `Producer::open` (needs a world-writable, non-sticky
  temp dir, or same-uid write access; a sticky `/tmp` blocks the unlink), the worker
  writes preview pixels through the symlink. This is outside the stated "same-user is not
  an adversary" line and I could not build a complete repro, so it is a question, not a
  finding. Opening the fallback with `O_NOFOLLOW` (and re-checking owner/mode) closes it
  without cost.
- **Q2 — `Store::load` aborts on a world-readable system fallback that holds `lanKey`.**
  `settings.rs` skips `lanKey`/`lanFingerprint` for non-primary paths, but `read_file`
  raises first when a fallback is group/other-readable and contains `lanKey=`
  (`settings.rs:350-373`). The whole load fails instead of skipping the credential. A
  misconfigured `/etc/xdg/nekomario28.conf` becomes a startup denial. Low; the fix is to
  drop the credential before the permission check on fallbacks.
- **Q3 — `settings.rs` writes a predictable temp name** `.Mobile-Webcam-<pid>.tmp`
  (`settings.rs:320`) in the config directory. `create_new` stops a symlink write, but a
  same-user pre-created file makes `save` fail (no retry). DoS only; mentioned for
  completeness.
- **Q4 — `ACTION_STOP_SESSION` is also not handled in `onNewIntent`.** When MainActivity
  is already resumed, a singleTop re-delivery takes the `onNewIntent` path, which ignores
  the action, so the notification's Stop is only reliable when the Activity is not
  resumed. That is the same code region as ID-001; it may be intended (the notification
  brings the Activity to front), but the asymmetry is not documented.
- **Q5 — the host trusts the phone's `VIDEO_CONFIG` for a large allocation** up to
  8192×8192 (bounded to 33,554,432 pixels, `video.rs:30-46`), i.e. ~134–268 MB per frame
  buffer (`pixels::convert`). Precondition is an authorized phone; I did not measure the
  transient footprint.

## What I did not examine

Windows DPAPI behaviour at runtime; any Android device/emulator behaviour (Gradle, JVM
suite, instrumentation, emulator conformance, screenshots); the GUI capture gate; the
release/Windows CI jobs; the actual TLS handshake or USB-AOA attachment; the
`io-budget`, `ffmpeg-sys-next`, `pipewire-vircam`, `rusb`, `rustls` and `ring`
implementation internals; the fuzz campaign; the Python CI TLS client; and the
`specs/`/`CHANGELOG.md` history beyond what the three prior reviews state.


---

## Disposition (implementer, 2026-10-23)

Every item below was acted on in the same session the review arrived. The report above is
left exactly as the reviewer wrote it.

### Findings, both fixed

- **ID-001 (any app can stop a live capture) — FIXED.** Verified the shape: the Activity
  is exported (it has to be: launcher entry and USB-accessory target), and
  `ACTION_STOP_SESSION` was accepted from any intent carrying it. The notification now
  carries a **random per-process token** (`UUID.randomUUID()`), and `consumeStopIntent`
  ignores an intent without it — a forged one is ignored rather than consumed, so the next
  genuine stop still works. The same call was added to `onNewIntent`, which also closes
  the reviewer's **Q4** (the button was dead while the app was in front, because
  `singleTop` re-delivery never reaches `onResume`). Two tests: the existing
  `the_notification_can_stop_the_session` was updated to the new shape, and a new
  `the_stop_action_is_not_a_credential_any_app_can_present` pins the token, its
  randomness, and both delivery paths.
  *Not verified on a device:* the replay of a forged intent. The path is static; the
  finding is a call path, and so is the fix.
- **ID-002 (unbounded IDR requests) — FIXED.** `VIDEO_IDR_REQUEST` now shares the shape of
  the PING limit it sat next to: a one-second window, at most **8** requests, and extra
  requests inside the window are **dropped rather than raised** (one recovery is already
  pending; a keyframe is coming). The asymmetry the reviewer measured is gone.
  *Not measured:* the bandwidth or battery difference; the fix bounds calls into the
  encoder, which is the claim.

### Tests that passed for the wrong reason — all four addressed

1. **Constant-time comparison guarded by a substring.** The comparison is now a
   behaviour, `SecureLanIdentity.authenticatesWith(expected, candidate)`, with a
   behavioural test (`WireAndQueueTest.pairing_code_comparison_is_exact`: right key,
   wrong key, short key, empty, reversed). The source-text assertion stays as a
   one-owner guard and now says in a comment what it cannot prove.
2. **The refusal-renderer test injected its own result.** A new crate test,
   `gui::tests::every_reason_code_reaches_a_rendered_title`, drives the **chain** —
   wire code → `client_auth::reason_text` → `refusal()` → rendered title — for every code
   including an unknown one. **It found a real gap on its first run:** `reason_text`'s
   fallback sentence ("the phone rejected this PC") was not classified, so an
   unrecognised code fell through to generic "connection failed" advice. Fixed, and the
   test now fails if a code is added without a title.
3. **The Area-C capture hypothesis had no test that attempted it.**
   `QualitySessionTest.aCommitAtIdleNeverStartsTheCamera` sends `PREPARE_QUALITY` then
   `COMMIT_QUALITY` to an authorized session that is not streaming, through the real
   controller, and asserts no capture start — including after a second tick, so a replay
   cannot land later.
4. **The preview symlink claim was a comment, and the tests exercised only the
   unprotected fallback.** That fallback is the path that normally runs (GUI creates,
   worker opens), so it now re-opens with `O_NOFOLLOW` and re-checks regular-file, owner
   and mode (`private_reopen`), and
   `preview::tests::a_record_replaced_by_a_symlink_is_refused` **asserts** the refusal for
   both a symlink and a widened mode, and that the symlink's target was not written.

### Questions

- **Q1 (preview record substitution) — fixed** as part of item 4 above. The reviewer's
  suggested remedy is what was implemented.
- **Q2 (`Store::load` aborts on a world-readable fallback holding `lanKey`) — FIXED.** The
  loader already *intended* to skip credentials from a system fallback, and it said so in
  a comment, but `read_file` refused the whole file first, so the two disagreed and the
  outcome was a startup denial caused by a file the user cannot edit. Fallbacks are now
  read with `read_loose_file`: credential lines are dropped and the rest supplies
  defaults. The user's own file keeps the strict rule (a readable key there still fails
  closed), and a test asserts both halves.
- **Q3 (predictable temp name for settings save) — FIXED.** The name was
  `.Mobile-Webcam-<pid>.tmp` with `create_new`, so a same-user process could pre-create
  it and make every save fail. The suffix now comes from the platform CSPRNG, and a test
  pre-creates the old predictable name and asserts that saving still works.
- **Q4 — fixed**, see ID-001.
- **Q5 (host trusts `VIDEO_CONFIG` up to 8192×8192) — measured, accepted.** The
  reviewer's precondition is an authorized phone sending a shape no phone produces (the
  app's own catalog caps at 3840×2160). Measured with
  `video::tests::largest_configuration_footprint` (ignored by default): at the accepted
  maximum, 8192×4096, the sink buffer costs **64.6 MiB of resident memory** with every
  page touched, plus a 48 MiB decoder frame by arithmetic — about **113 MiB transient on
  Linux, 192 MiB on Windows** where the sink layout is RGBA. That is acceptable on a
  desktop, so the bound stays and is now evidence-backed rather than argued; the
  measurement also produced its own lesson (the first version of it did *not* touch the
  pages and reported 2.6 MiB for a 64 MiB buffer — a lazy allocation that looks small).
