# Changelog

## Unreleased — per-PC authorization (independent review returned a merge blocker)

- Fix the three findings of the second security review, in its order. `HOST_HELLO` no longer
  documents a re-send that no code path can reach, and no longer renews a proof deadline this
  file pins as absolute — the rule now matches the wire (a session that has already challenged
  ends, one that has not yet challenged ignores the hello), with a test that pins *that*.
  `closeAndAwait` keeps the reader handle when the reader survived, `readerAlive()` answers
  whether it is still running, and the window remembers the object whose reader refused to stop —
  so the single-reader refusal is durable instead of one iteration deep. And `sendPairingInfo` is
  gated on `permitted()` like every other sender, with the action offered only for an authorized
  session, because that is what the hand-off is for.

- A second independent security review. A fresh session (same model, no prior context) was
  pointed at what changed since the first pass — the `HOST_HELLO` frame, the single-reader
  handover, the phone's rate limit and the host's pacing, the new CLI surface, stream alignment
  — and returned **three defects, all Low, all inside the fixes the first review prompted**. It
  also names five tests that would pass for the wrong reason. Report, disposition and the repair
  plan: [docs/reviews/independent-review-2026-10-24.md](docs/reviews/independent-review-2026-10-24.md).

- Accept the quality switch on a physical phone. Gate #8 wanted device verification of the
  resolution and frame-rate controls and there was no repeatable way to ask for a change: the
  GUI is the product but a script cannot click it. `--headless --quality WxH@fps` asks once
  streaming has started and exits 1 unless the phone confirms that exact mode; the phone
  applies it through the real capture path (the worker drives the staged apply and the commit).
  Measured on the XQ-GE44: **`640x480@30` passed and `1280x720@15` passed** — one changes both
  axes, the other changes only the frame rate. Evidence:
  [docs/evidence/2026-10-24-quality-switch-acceptance.md](docs/evidence/2026-10-24-quality-switch-acceptance.md).

- Say what to do when the phone's address changed. Wi-Fi guidance used to end at "check the
  address shown on your phone", which is a dead end when DHCP has moved it (we hit `.14` →
  `.15` in this session and lost a diagnostic round to it). It now names the one-tap fix: the
  cable, then *Send Wi-Fi pairing to PC* on the phone, which updates the stored address — the
  path that already existed but was never suggested at the moment it was needed.

- Say how to stop asking. The Android dialog that asks whether the app may use the
  accessory has an **always allow** option; ticked once, the grant is remembered for that app
  and accessory and later plugs need no tap (verified by the user on the device). The status
  line now says which box, and the USB instructions say it too. It is cleared by reinstalling
  or updating the app, which is what made it look like a per-connection cost during
  development — every iteration here installed a new debug APK. Our AOA identity strings are
  fixed by `specs/rust-host-rewrite/contracts.md`, so the accessory the system remembers is
  always the same one.

- **G0.5 passes at 1000 round trips.** The USB gate now completes: 1000 sequences matched,
  the 8-byte challenge echoed on every exchange, 0 mismatches, 0 timeouts, 0 disconnects,
  rtt p50 550 µs (paced). The blocker turned out to be the instrument — the phone rate-limits
  `PING` to 64 per second and refuses the session when a peer exceeds it, so a probe sending
  ~2000 per second was killing its own session every ~60 exchanges (measured: 576 round trips
  across nine seams, 0 mismatches — a healthy link). `--rate` now paces at 32 per second, which
  also explains the earlier `5 FAIL / 60 PASS / 1000 FAIL` bracket. Two further host fixes were
  needed first: the stream is aligned at connect (the accessory endpoint holds a previous
  connection's frames and AOA reports no disconnect), and a spent accessory session can be
  reset (`HOST_HELLO` plus `reset_stream`), because the phone sends one challenge per session
  by design.

- Let a reconnecting host start a fresh exchange. USB has no session boundary and AOA
  reports no disconnect, so a host that reconnects found the accessory session the previous
  connection had used — quiet if it had authorised somebody, or answering with the older
  connection's frames — and the only way out was restarting the app or replugging. The host
  now says `HOST_HELLO` (0x0e, zero payload) when a failure looks like a spent session, and
  retries the exchange once; the phone answers by re-sending the same challenge if it is
  waiting for a proof, or by ending the session if it has already answered. No identity
  changes hands and a phone that does not know the type yet ends the session and rediscovers
  — the behaviour the frame exists to produce. Documented in
  `protocol/client-authorization-v2.md`; both sides have tests.

- Stop two readers sharing one accessory. On USB the previous session's reader and a
  new session's reader sit on the same endpoint, and a blocked read is not interruptible —
  closing the descriptor is what unblocks it. `openAccessory` closed the old session and
  then opened the new one without checking that the old reader had actually gone, so an
  old reader could take the new connection's proof, answer it against a challenge the phone
  had already replaced (the phone says `BAD_SIGNATURE`) and leave capability frames in front
  of the authorization result (`expected authorization outcome before capabilities/media`).
  `closeSession` now reports whether the reader stopped, `AccessorySession.closeAndAwait`
  answers it, and `openAccessory` refuses to open a second reader and tells the user to
  replug instead — a wiring test pins all three. Both symptoms were measured on a real phone
  (issue #78), intermittently.

- Give the USB gate a driver, and fix what it found. The gate for the AOA path (`G0`,
  `G0.5`) had no way to run: the `PING` type existed as a constant and a wire test, the
  Android app answers it, and no shipping code ever sent one. `mobile-webcam --usb-probe
  [--frames N] [--budget-ms N]` now performs the transition, the authorization and N
  synchronous round trips with the sequence and payload checked on every one, printing RTT
  percentiles and exiting 0/1/2; `--usb-dump` and `--usb-frames` show the raw transfers and
  the frames the reader parses, which is how the defects below were found. Measured: **G0
  passes** (the phone re-enumerates as `18d1:2d01`) and **G0.5 passes at 50 round trips**
  (rtt p50 80 µs, nothing mismatched or timed out) in 2 of 3 attempts, not yet at the 1000
  the gate asks for.
- Three host defects fixed, each found by running the gate: the accessory endpoint can hold
  frames from a previous host connection (AOA reports no disconnect), so the reader started
  mid-frame and could never complete one — `Usb::connect` now aligns the stream and keeps
  the first header it finds instead of discarding a live frame; those stale bytes also made
  the host sign a challenge the phone had already replaced, which the phone answers
  BAD_SIGNATURE, i.e. a framing problem reported as a key problem; and a reply check that
  treated the phone's capability advertisement as the answer to every PING. Evidence:
  [docs/evidence/2026-10-24-usb-g0.md](docs/evidence/2026-10-24-usb-g0.md).

- Measure the question the two-hour item was really about. Ten minutes on the phone with
  the screen **off** (`Dozing`): **17 851 frames at 30.46 fps**, hardware (vaapi) decode,
  `submitted == decoded` in every one of 587 samples, thermal +4.5 C battery / +7.8 C CPU.
  So capture survives the screen turning off on this device — the foreground service with
  the camera type is the mechanism, and `FLAG_KEEP_SCREEN_ON` was never the thing deciding
  it. Recorded in
  [docs/evidence/2026-10-24-phone-soak-screen-off.md](docs/evidence/2026-10-24-phone-soak-screen-off.md);
  energy remains unmeasured because the phone was on a charger, which the instrument says
  rather than reporting the charger's draw. The instrument also gained a `SCREEN_OFF` mode
  and a per-row frame rate from the host's decoded counter — the first version counted log
  lines and printed `fps=1.0` for a healthy 30 fps stream.

- Retry the pinned FFmpeg fetch instead of asking for a rerun. The Windows job failed
  three times in one day on `URLError: Connection timed out` while fetching the source
  tarball — twice on a pull request and once on `main` — and passed on every rerun. The
  URL and the sha256 are pinned and a partial download is removed before each attempt, so
  three attempts cannot fetch something else; measured: an unresolvable host fails after
  3 attempts in 15 s and leaves no partial file for the next attempt to trust.

- Close the review's three remaining questions. The world-readable system fallback no
  longer denies startup (the loader already intended to skip credentials from a fallback;
  `read_file` refused the file first, so the two disagreed — a fallback now drops the
  credential and supplies the rest, while the user's own file still fails closed on a
  readable key). The settings save's temporary name comes from the platform CSPRNG
  instead of the pid, so a same-user process can no longer pre-create it and block every
  save. And the largest accepted `VIDEO_CONFIG` is measured rather than argued:
  **64.6 MiB of resident memory** for the sink buffer at 8192×4096 with every page
  touched, about 113 MiB transient on Linux and 192 MiB on Windows, so the bound stays —
  with the measurement's own lesson recorded (an untouched 64 MiB allocation reported
  2.6 MiB until the pages were touched).

- Independent security review, and its findings fixed the same day. An independent
  session (`aiand/deepseek-v4.1-flash`, no prior context) reviewed the areas the request
  named and returned two real findings, both fixed: **any installed app could end a live
  capture** through the exported Activity's unauthenticated stop action (the notification
  now carries a random per-process token, and the action is handled on both intent
  paths), and **`VIDEO_IDR_REQUEST` had no rate limit** while the `PING` beside it had one
  (now 8 per second, extras dropped). It also named four tests that were passing for the
  wrong reason — a substring assertion standing in for the constant-time comparison, a
  renderer test that injected its own result, an Area-C hypothesis with no test that
  attempted it, and a symlink claim asserted only in a comment — all four replaced with
  behavioural tests. One of the replacements **found a further gap**: an unrecognised
  refusal code rendered as generic "connection failed" advice. The preview record now
  re-opens with `O_NOFOLLOW` plus an owner and mode re-check. Report, evidence boundary
  and per-item disposition:
  [docs/reviews/independent-review-2026-10-23.md](docs/reviews/independent-review-2026-10-23.md).

- Update the phone, and measure a real stream. The APK on the device was still
  `0.1.4` from the previous release line; `main` builds `0.2.0`, and installing it
  showed the new UX working on the device — `許可したPC（3台）`, the receiver named while
  streaming (`yuu へ配信中（申告名）`), and the pairing key, key rotation and "erase
  authorization data" all behind Details. Then a ten-minute stream attempt:
  **11 609 frames at 30.47 fps, 1280x720, hardware (vaapi) decode, nothing dropped**,
  with battery +2.1 C, CPU +14.4 C and skin +4.5 C before the phone left the USB bus at
  6 min 27 s and the host reported a connection reset. Battery drain was not measurable
  (the phone was on AC); the record is in
  [docs/evidence/2026-10-23-phone-soak.md](docs/evidence/2026-10-23-phone-soak.md).

- Soak to the owner's bar: ten minutes. 600 s and 18 115 frames through the real frame
  loop leave resident memory settling at **11.3 MB** (down from 27.9 MB as the allocator
  returns pages) and throughput flat at 30.19–30.23 fps across forty samples. The
  two-hour item becomes a ten-minute one, and what is left of it needs the phone:
  battery, temperature, and whether capture survives the screen going off.

- Run a session without a window. `mobile-webcam --headless [--transport usb|lan]
  [--host ADDRESS] [--seconds N]` starts the same worker the window starts, prints every
  control event as one JSON line, and exits **0** if it streamed, **1** if it did not,
  **2** if it could not start — so a script or a CI job gates on the run instead of
  scraping a log. Measured: against a phone that is not there it prints
  `connecting → error: Connection refused → stopped` and exits 1.

- Correct the ledger where it was wrong, and soak the host. `FLAG_KEEP_SCREEN_ON` applies
  only while its window is visible, so a backgrounded session does not hold the screen on
  — the earlier note claiming a two-hour stream keeps a two-hour screen awake was wrong,
  and the real open question is whether capture survives screen-off at all. Added an
  ignored soak test: five minutes at 30 fps through the real frame loop leaves resident
  memory **flat at 28.0 MB** and throughput at **30.25 fps** throughout. Its CPU column
  was removed rather than published: it read 0.0 s for a process that had decoded for
  minutes, which is what a broken instrument looks like.

- Read the phone side for repeat work. One real find: every accepted Wi-Fi connection
  read the certificate from the Android Keystore, hashed it, formatted the digest as 64
  hex characters and parsed that hex straight back into bytes, with the access key
  hex-decoded per connection as well. The identity now caches the certificate and the
  decoded key (refreshed on rotation) and answers with the digest itself; a wiring test
  fails if the session goes back to `fingerprint().chunked(2)`. The rest of the phone is
  recorded in [docs/performance.md](docs/performance.md), including the largest energy
  item — `FLAG_KEEP_SCREEN_ON` for the whole session, kept because screen-off capture is
  unmeasured rather than because it is free.

- Measure the pipeline itself, not just what was added to it. An ignored in-crate test
  streams a committed 90-frame 1280x720 H.264 fixture through the real frame loop with
  a counting sink: **decode 0.22 ms, swscale 0.61 ms, whole loop 1.47 ms per frame**
  (660–680 fps, three interleaved runs) — **about 4.4 % of one core at 30 fps**, i.e.
  22x the headroom the frame rate needs. The ledger says what that excludes: the sink's
  copy into a device buffer, hardware decode, the phone and the link.

- Audit what repeats, and write the ledger down. `docs/performance.md` lists every
  per-second path with a five-run median: a received access unit costs 0.45 µs, the
  preview conversion 366 µs, its publish 5 µs, the GUI's preview image 112 µs — about
  0.5 % of one core together. Two optional trims are named with their ceiling (skip the
  preview conversion when nobody watches, 0.37 %; reuse the image buffer, 0.11 %) and
  refused with reasons. The earlier single-run 320 µs figure is corrected to 366 µs
  median, spread 343–373: it was an outlier.

- Say when the next retry lands. "Waiting for the phone (retrying)" could not be told
  apart from "stuck", so the waiting line now carries the attempt and the deadline:
  `スマホを待っています (2回目/8・4秒後に再試行)` / `Waiting for the phone (retry 2 of 8
  in 4 s)`. The wording is pinned by a test, including a deadline that has already
  passed.
- Measure what the preview costs, since it converts at 10 Hz whether or not anyone is
  watching: **320 µs per frame and 5 µs to publish — 0.33 % of one core** at 1280x720
  and 1920x1080 (both scale to 320x180), measured in a release build with
  `examples/preview_cost.rs` and recorded in
  [docs/evidence/2026-10-21-preview-cost.md](docs/evidence/2026-10-21-preview-cost.md).

- Write the review request. `docs/reviews/request-2026-10-21.md` turns the gap list
  into work: four unread areas with their file lists and line counts, the facts the
  implementer read about the Android manifest and the capture-consent boundary —
  labelled as pre-work to distrust — and one testable hypothesis per area, including
  "reach `startCamera()` with `cameraStartRequested` false by any sequence of host
  frames". It also asks the reviewer to attack the Evidence column, because this
  repository has twice had a test that passed for the wrong reason.

- One version, and a gate that holds it. `VERSION` said **0.1.4** while
  `host-rs/Cargo.toml` said **0.2.0**: the release names artifacts from `VERSION`
  while the binary reports Cargo's, and `release.yml` refuses a tag that does not match
  `VERSION` — so a release cut from this tree would have failed its own first check.
  `VERSION` is now 0.2.0 and `host-rs/tests/version.rs` fails when the two disagree,
  plus a test that the next build's `versionCode` out-ranks the field build's 1004.
- Map the security work. `docs/reviews/security-coverage.md` records which review
  covered what, which test proves each claim, the trust assumptions of everything added
  since those reviews (the USB pairing hand-off trusts the cable; the preview record is
  a 0600 file in the temp directory; the PipeWire node is session-readable), and the
  list of components nobody has read. It is written by the implementer and says so.

- Add the phone-side recovery for authorization data that cannot be read. The
  storage-failure screen offers one control, pressed twice (the label states what
  the second press does), which clears the encrypted record and this namespace's
  Keystore key; nothing resets automatically and partial failure still fails
  closed. The device conformance scenario now presses it and requires the effect —
  record and key gone, a fresh store readable — and the app logs the outcome under
  the tag the workflow already collects, so a failure names its reason.
- Make recovery reachable when the record cannot be read: `ClientStore.clear()`
  clears the record and this namespace's key *without* reading either. Constructing
  a store decrypts the record, so `ClientStore(context).reset()` threw
  `AEADBadTagException` before it could clear anything — measured on the device, with
  the record and its key still in place afterwards. The device test now corrupts a
  record and requires it to be cleared, with a fresh store reading as never-used.
- Fence writers across a deliberate reset: an owner created before the reset can no
  longer publish, so a live session's expiry or revocation cannot resurrect the
  erased record. Measured on the emulator — without the fence the reset returned
  without an error and both the record and its key were present again immediately,
  which made the recovery action look ineffective.

- Show the preview. While streaming, the PC window carries a preview of what the
  phone is delivering, so framing yourself needs no second app — on Linux and
  Windows, and even before anything has attached to the virtual camera. The worker
  publishes it at a fixed 10 Hz through a small private seqlock record, so the
  window's repaint rate no longer decides how often the picture changes, and a
  half-written frame is dropped rather than shown torn. The capture gate renders it
  from a committed fixture so the preview cannot disappear unnoticed.

- Say who is receiving. While streaming, the phone's status names the PC
  (`yuu へ配信中（申告名）` / `Streaming to yuu (self-declared)`) instead of a bare
  "streaming". The name is marked as the PC's own claim; the verified identity stays
  the client id shown at approval and in Authorized PCs.

- Put the numbers where the question is. The phone's `Authorized PCs` button now
  says how many are approved ("none yet", "2"), and the pairing key hides itself as
  soon as the first PC is approved — it exists to get that approval, and afterwards
  it is clutter and a secret with nothing to do (the button brings it back). The PC
  window opens tall enough (420x620) to show its status, the guidance and, while
  streaming, the mode controls without scrolling first.

- A Linux camera that needs no kernel module. `--features pipewire` adds a PipeWire
  `Video/Source` backend (`outputKind` in settings, switchable under Details): no
  `v4l2loopback`, no `pkexec modprobe`, the same frames — with the honest caveat, in
  the UI, that only PipeWire-aware applications list it today. Verified against a real
  consumer: node `mobile-webcam` in `pw-dump` as `Video/Source`, 60 buffers captured by
  `gst pipewiresrc`, and the probe's five bars arriving as red, green, blue, pink and
  grey at 640x480.

- Measure whether the tests catch bugs. `cargo-mutants` now runs weekly over the
  preview record, and its first two rounds are the point: **185 mutants, 137 caught
  (77%)** showed that loose bounds (`g < 40` hid a ±11 colour shift) and uniform
  fixtures (identical pixels make index arithmetic invisible) were propping up the
  suite. With exact-value assertions and fixtures that carry their own coordinates,
  **159 of 185 are caught (89%)** and the rest are equivalent mutants around the
  sequence number. The record's positional I/O also moved to `preview/posio.rs`, so
  a mutant in the platform shims — which the other platform cannot even compile —
  does not distort the number.

- Fuzz the decoders. `host-rs/tests/parsers.rs` now runs every parser reachable from
  the network — header, capabilities, quality, client challenge/auth/result, epoch,
  pairing info and video config — over arbitrary bytes as a property test on stable,
  and `host-rs/fuzz/` does the same under libFuzzer on a weekly schedule. First run:
  **19.6 million executions in 180 s, 513 edges covered, no crashes**, with the corpus
  committed so the next run continues from it. That is the precondition the security
  review would otherwise have had to take on trust.

- Take three of the survey's suggestions. The host window now has real UI tests:
  `egui_kittest` drives the actual widget tree through AccessKit, so "a paired host
  hides the pairing fields", "clicking Details reveals them" and "a revoked PC is told
  it was revoked" are assertions instead of screenshots. `zizmor`, `typos` and an
  offline `lychee` link check are gates; the first pass pinned third-party actions to
  commits (GitHub's own `actions/*` stay on tags by policy), stopped the release job
  from reading a Gradle cache a pull request could have written, and taught the typo
  gate the kernel's `S_PARM` and NSIS's `StrCpy`.

- **Security:** `rustls-webpki` 0.103.15 → **0.103.16** (RUSTSEC-2026-0382, "certificate
  verification may accept distrusted intermediates"). The advisory is newer than the
  last lockfile change, so the audit only saw it when this branch touched
  `Cargo.lock` — every TLS handshake this app makes went through the affected code. The host window now has real UI tests:
  `egui_kittest` drives the actual widget tree through AccessKit, so "a paired host
  hides the pairing fields", "clicking Details reveals them" and "a revoked PC is told
  it was revoked" are assertions instead of screenshots. `zizmor`, `typos` and an
  offline `lychee` link check are gates; the first pass pinned third-party actions to
  commits (GitHub's own `actions/*` stay on tags by policy), stopped the release job
  from reading a Gradle cache a pull request could have written, and taught the typo
  gate the kernel's `S_PARM` and NSIS's `StrCpy`.

- Survey what other OSS projects could be taken from. `docs/upstream-options.md`
  records the crates and projects worth adopting (headless egui tests, a PipeWire
  camera that needs no kernel module, mutation and fuzz gates, JVM phone screenshots,
  workflow static analysis), the ones rejected and why — including the AGPL/GPL
  projects that are reference-only — and the order to take the first ones in.

- The pairing key is a fallback, so it is no longer on the main screen. A PC on a
  USB cable takes the fingerprint and key itself (`Details → Send Wi-Fi pairing to
  PC`), so the phone stopped pushing them at the user: the reveal control, the key
  block, the key rotation and "erase authorization data" now live behind Details, and
  nothing reveals the key on its own any more. The how-to leads with the cable and
  names the manual path second. Measured while doing it: `updateControls` set the key
  button's visibility itself and overruled the disclosure, so the button was back on
  the main screen with every test green — the rule now has a single owner and a test
  that fails if a second one appears.

- Take the repeated lines off the screen. The applied capture target said what the
  mode line above it already said; "edit or remove the pairing under Details" was
  restated on every visit; and "Preview" labelled a picture that plainly is one. All
  three are gone. The preview is now also shown only while frames are arriving, so a
  stopped stream cannot leave a frozen image beside a status line that says the
  camera is waiting — the capture gate gained a `stopped` state so that stays true.

- Write the roadmap down. `docs/roadmap.md` records where each area stands and what
  evidence backs it, the three things that block a release (independent security
  assessment, version/edition reconciliation, real-phone switching acceptance), and
  for every remaining item the test that would close it.

- Keep the window awake while a session runs. Measured on the real phone: after the
  phone stopped the camera the PC still said "camera connected", because the UI only
  woke for events and no more arrived. A running session now requests a repaint every
  100 ms, so the preview follows the worker's 10 Hz record (measured: consecutive
  frames 0.00 apart before, 1.8–3.2 after) and a stop shows up immediately as the
  waiting state it is.

- Report silence. When the phone stops the camera the session stays up and frames
  stop, so the PC used to say "camera connected" indefinitely; after three seconds
  of silence outside a quality switch it now says no video is arriving and that the
  camera may have been stopped on the phone. The paired line also shows when this PC
  last really streamed (`最終接続: 3分前` / `Last connected: 3 min ago`), so "it worked
  yesterday" is distinguishable from "it has never worked".

- Say which refusal it was. The phone already answered with a reason — revoked,
  expired, too many PCs, signature rejected — but the PC reported every one of them
  as "access key rejected", sending the user to re-check a key that is not the
  problem. Each reason now has its own title and its own one-line instruction
  (approve again, renew, remove a PC, replace the PC key), from a single classifier
  both come from.

- Put the stream on the main screen. While a stream runs the PC now shows the
  confirmed mode and the device to pick in other apps
  (`1280×720 · 30 fps → /dev/video10`), with resolution, frame rate and Apply right
  there — changing resolution mid-call is the ordinary action, not an advanced one.
  The rest (output path, decode, rotation, mirror, counters) stays under Details,
  which also keeps the mode controls before a stream exists. The phone's pairing
  block now says that a USB cable makes the typing unnecessary.

- Pair over the cable instead of typing 96 hex characters. On a USB session the
  phone can send its Wi-Fi details (address, certificate SHA-256, 128-bit access
  key) as a `PAIRING_INFO` frame; the PC stores them exactly as if they had been
  entered, shows what it received, and moves to Wi-Fi on one click. It is not an
  authorization (the phone still approves each PC) and the button is explicit,
  because it puts the access key on the cable — the same trust limit USB already
  has. Found while doing it: the Wi-Fi key-rotation control had been built and
  given a visibility rule but its `addView` had been dropped, so it was
  unreachable; a wiring test now requires every control to be in the layout.

- Wait for the phone instead of asking. With a complete pairing the PC connects at
  launch, and if the phone is not answering yet it retries with a bounded backoff
  (2 s, 4 s, 8 s, then 15 s; eight attempts) while the status reads
  `Waiting for the phone (retrying)` — a phone that has not been opened or has just
  been restarted is the ordinary case, and it used to need a manual Connect.
  Failures no retry can heal (rejected key, fingerprint mismatch, recovery
  required) stop immediately, and Disconnect suppresses retries until the next
  press. Verified with a listener that appears after the first attempt: the app
  connected to it on its own.

- Show state, hide procedure. Both screens now say what is happening and keep
  the rest one disclosure away: the phone's status is a short state rather than an
  address plus a how-to, the key rotation (which disconnects every Wi-Fi client)
  sits behind Details with the address and instructions, and the PC shows
  `Paired with <address>` with the pairing fields, the how-to and "Forget paired
  phone" inside Details. Steps without a decision are automated: the phone reveals
  its pairing key while no PC is approved, and the PC connects on launch when a
  complete pairing is stored. Consent is untouched — the phone still approves each
  PC and still owns the first camera start. The GUI state-capture gate found that
  two fixtures had been distinguishing themselves by a stray USB line rather than
  by a real difference; the fixture now applies a different mode so the switch is
  visible, and 12 captures pass with 0 indistinguishable pairs.

- Keep streaming while the phone is used for something else. The camera now runs
  inside a `camera`-type foreground service whose ongoing notification says it is
  live and carries the action that stops it, and `onPause` no longer stops the
  capture (`onDestroy`, the app's Stop, the notification's Stop and authorization
  loss still do). Measured on a physical phone: before this, pressing Home closed
  the session and the consumer read 0 frames. A wiring test pins both halves and
  fails on the previous `onPause` (2 of 112 tests fail against that mutant).

- Run the conformance on a physical phone on purpose. The instrumentation still
  refuses a device by default, but `-e physicalDevice true` designates one, and
  `android/tools/physical-conformance.sh` wraps it so the display, font scale,
  locale and screen timeout are restored on *every* exit including SIGINT and
  SIGTERM — an aborted hand-run left `Override size: 840x1400` on a physical
  phone until it was reset by hand. The camera acceptance and the storage/TLS/UI
  conformance both ran on a physical XQ-GE44 (Android 16, 77 of 77 advertised
  modes, `c2.qti.avc.encoder`) and the marker says which kind of camera the
  evidence came from.

- Make the recovery command's arguments say what they mean. `--replace-identity`
  is what a user runs when the app will not start, so a mistyped flag used to open
  the GUI that cannot start for the same reason; unknown options, a stray
  argument, and `--force` without `--replace-identity` are now named and rejected
  (exit 2) instead of being ignored, and `--help` prints the three forms.
  `--replace-identity` no longer creates a key while refusing to replace one
  (`load_or_create` published one when none existed, which also made "nothing was
  replaced" false), and a concurrently guarded quarantine name is reserved before
  the rename rather than checked first, since `rename` overwrites.

- Add the explicit host recovery path for an unusable stored PC key. Startup
  still fails closed and leaves the file untouched; `--replace-identity`, and the
  same action in the GUI behind a two-step confirmation, rename the unusable file
  to `<name>.corrupt-<unix-seconds>` (bytes and permissions preserved, never
  overwritten) and publish a new key, saying that every phone approval for this
  PC is invalidated. A key that still reads is refused without `--force`.

- Record the exact three blockers that stop the local Gradle JVM gate on a fresh
  machine (Gradle 8.9 rejects a Java 27 JDK; the project's Java 17 toolchain must
  be named in the *user* Gradle properties, a command-line `-D` is not enough;
  the SDK needs `build-tools;34.0.0` and `platforms;android-35` with licences
  accepted). With them, `./gradlew :app:testDebugUnitTest` runs 106 tests with no
  failures and `:app:assembleDebugAndroidTest` builds the instrumentation APK
  locally, so the repository's own toolchain — not a cache-only probe — is the
  local evidence.

- Add `host-rs/tools/capture-gui-states.sh`: it captures the desktop GUI's
  fixture-driven states for both locales through the native capture entry point,
  writes a hashed manifest, and fails when two fixtures render the same frame so
  a state mapping cannot regress silently. A fixture session also reports itself
  running now, and a details capture gets a window tall enough for the quality
  controls, so the captures show the applied target, output device, rotation and
  stats rather than a clipped frame.

- Move the pinned `io-budget` revision forward to its `main` (`65df550`), which
  fixes the deadline layer handing a released stream back with a stale
  budget-derived socket timeout instead of the pre-wrap values (its issue #1)
  and takes `io::Result` end to end. The three transports adapt to that error
  type at the framing boundary; a libusb failure there is carried as an
  `io::Error` rather than a boxed trait object.

- Produce the Linux virtual camera through the V4L2 streaming API instead of
  `write(2)`: request MMAP output buffers, fill and queue them with a monotonic
  timestamp, and drop a frame when no consumer has released a buffer. Measured
  with one producer and one reader per freshly created `v4l2loopback` device: the
  shipped `write(2)` producer leaves the device output-only, so a reader cannot
  even open it, while this producer feeds an `ffmpeg` reader **74 frames /
  306 KB**, against **50 frames** for an `ffmpeg` producer as the control.
  `exclusive_caps=1` and the shipped modprobe configuration are correct and
  unchanged; a device instance that has already served other producers can end up
  serving nothing until it is reset, which issue #18 tracks separately.

- Extend the device acceptance to a mid-stream quality switch on a real Camera2
  camera: PREPARE, exactly one pause barrier, COMMIT with the next epoch, an
  applied result, a new VIDEO_CONFIG and keyframe for the target mode, then live
  access units — followed by revocation, which still stops capture and releases
  the camera. The protocol doc now states that a Commit with a stale epoch is
  ignored without a reply, which this execution reproduced.

- Close the review's UI-wiring guard gap: the authorization producer stop is now
  `CaptureCancellation`, a production class that owns its own daemon thread and
  cannot be pointed at the main looper. `MainActivity` holds the single instance,
  the device acceptance constructs the same class, and two guards (a contract
  test for the threading plus a wiring test over `MainActivity.kt`) were both
  observed failing on a throwaway commit that restored the pre-repair
  `handler.post` stop.

- Record the independently reviewed authorization checkpoint: one Medium blocker,
  AUTHREV-001, because Camera2/MediaCodec teardown still depends on the main
  thread rather than the authorization lifetime, so capture can outlive expiry or
  revocation until the UI thread runs. Output remains gated, but the required
  non-UI producer-cancellation path is not implemented. Merge is blocked until it
  is, with a guard that fails on the old state.
- Record AUTHREV-002: persistence holds the shared authorization monitor, so a
  stalled Keystore/commit call delays expiry and owner close. The owner
  reproduction failed closed and is documented as delayed teardown, not a bypass.
  See docs/reviews/per-pc-authorization-review.md for scope, evidence and limits.
- Second independent review of the repair found that moving persistence off the
owner monitor had made revocation stop taking effect until the write returned
(REV-001) and a cancellation window while the camera session was constructed
(REV-003). Revocation now takes effect in memory before the durable write, and
`start`/`close` of the camera bridge are serialised; both keep a regression, and
REV-001's guard was observed failing on the pre-repair behaviour. Remaining
review items (UI-wiring guard gap, unbounded platform calls on the teardown
thread, MediaCodec call sites, an Allow that persists while its request expires)
are recorded as accepted limitations in the repair review record.
- Repair both findings. The session now takes a required capture-cancellation
  sink, stops the Camera2/MediaCodec producer off the UI thread and refuses to
  install repeating capture when authorization already ended. Authorization
  persistence runs outside the owner monitor with wait-free published snapshots,
  so a stalled Keystore/commit call no longer delays expiry or teardown. Each
  repair keeps a regression that was observed failing on the pre-repair
  behaviour; the Android teardown itself still needs device confirmation and has
  not been independently re-reviewed.

- Add real user-scoped PC identity persistence with atomic first-use publication,
  Unix private-file checks and Windows DPAPI ciphertext. Add a validated,
  immutable phone authorization repository and Android Keystore AES-GCM storage.
  Both transport startup paths now consume the stores through one phone owner.
- Encode raw P-256 signatures as minimal DER for the phone verifier; cover
  leading-zero scalars with shared Rust/JVM/Android instrumentation fixtures.
- Reject invisible format controls and line/paragraph separators in labels by
  Unicode scalar, including supplementary tags that UTF-16 character checks missed.
  Reuse the already-locked Unicode category crate on the host.
- Reject malformed identity text without a UTF-8 slicing panic. Preserve corrupt
  stored identities for explicit recovery instead of silently replacing them.
- Require fresh per-PC proof before capabilities, controls or output on USB and
  Wi-Fi. Replace unshipped auth v1 with public-key enrollment and a domain-separated
  v2 transcript bound to transport, one-use nonce, point, label and the pinned
  phone certificate on LAN. No empty-list/bearer-only fallback.
- Bind pending action views to one session, distinguish stale actions from
  storage failures and retain prompt cooldown after disconnect/abandonment.
  Refuse a missing authorization record when its Keystore alias survives.
- Add EN/JA phone approval and saved-PC renewal/revocation, live expiry/revoke
  closure, bounded pending requests and a responsive PC wait/Stop with full ID.
  A grant never starts the camera. USB needs no QR but has no cryptographic phone
  attestation or active-physical-relay guarantee.
- Extend emulator conformance with production-Activity pointer actions, persisted
  approval/renew/revoke effects and narrow/scaled captures. Runtime/visual results
  must be checked at the exact revision; tests do not replace independent review
  or physical USB/camera/native-consumer acceptance. Match Android's transformed
  dialog button text in the pointer driver, and keep Android dimension getters
  out of the shared media boundary so the retained socket case runs in Gradle's
  plain JVM suite rather than only with android-all utilities.
- Close an unstarted LAN session's socket during shutdown and refuse restarting
  that closed connection; retain a real-socket EOF/restart regression.
- Upgrade the Android app and Rust host together: older clients, including the
  classic Qt host on USB, do not implement mandatory v2 authorization.

## 0.1.2 - 2026-10-04

- Add an experimental Windows host and a single Setup.exe that installs the app,
  runtime libraries and Unity Capture virtual camera, with a Start menu uninstaller.
- Remember the desktop Wi-Fi IP, transport, decoder and orientation settings.
- Combine USB accessory switching and receiver startup into the desktop Start
  action, and keep the selected phone through USB re-enumeration.
- Let Android USB camera startup wait for the PC connection; combine camera
  start, stop and cancellation into one button and remember the transport.
- Keep Android connections through screen rotation and put camera information
  behind Details.
- Report waiting and streaming separately; stopping the desktop receiver no
  longer blocks the interface.
- Bundle libusb and the one-time camera-access setup in the Linux AppImage.
  Setup enables boot-time module loading and avoids unloading other cameras.

## 0.1.1 - 2026-09-17

- Use the system language for a simple English/Japanese interface.
- Remove the Wi-Fi PIN; LAN connections now use a minimal AMB1 handshake.

## 0.1.0 - 2026-09-17

- Launch the project publicly as **Mobile Webcam** while retaining AMB1 as the
  internal wire protocol.
- Stream Android Camera2 -> MediaCodec H.264 to Linux over USB AOA or Wi-Fi TCP.
- Pair LAN sessions with a generated six-digit PIN and one active client.
- Reuse bounded latest-live video queuing and IDR/config recovery on both
  transports.
- Add FFmpeg hardware decode selection with VAAPI/CUDA support and automatic
  software fallback when hardware initialization is unavailable.
- Provide a Qt 6 desktop GUI for transport, pairing, decode mode, V4L2 output,
  0°/90°/180°/270° live rotation, and independent horizontal/vertical flips.
- Package the Linux GUI as `Mobile_Webcam-<version>-x86_64.AppImage`.
- Add a localhost TCP integration test covering pairing, framed traffic, IDR
  request, and wrong-PIN rejection.
