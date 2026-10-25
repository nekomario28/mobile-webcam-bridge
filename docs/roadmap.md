# Roadmap and TODO

What is left, in the order it should be taken, with the test that would let a claim
be made. Everything in "where it stands" was verified on `main` (2026-10-21) and, for
the device rows, on a real XQ-GE44 over Wi-Fi.

## Where it stands

| Area | State | Verified by |
|---|---|---|
| USB transport (AOA) | works end to end on the previous release lineage | gate G0/G0.5 history; **not re-run on this branch** |
| Wi-Fi transport | TLS 1.3, pinned phone certificate, 128-bit key, per-PC approval | emulator gate + negative-path matrix + real phone (2026-10-21) |
| Authorization | approval prompt, 90-day grant, revocation with distinct reasons, no re-approval on reconnect | Android unit tests, emulator gate, real phone |
| Video | 1280x720 30 fps into `/dev/video10` (yuyv422) | real phone: `ffprobe` + 60 frames in 2 s |
| Quality controls | capability discovery, staged apply, busy/failure states | Android unit tests, emulator semantic marker, synthetic peer matrix; **real-phone switch not re-run on this branch** |
| PC GUI | main screen shows state only; procedure and destructive actions under Details; preview; last-connected stamp | capture gate (10 states, 0 indistinguishable), real-phone screenshots |
| Preview | worker publishes a 10 Hz record; window reads it; works before any app attaches | seqlock + worker tests, capture gate fixture, real phone (frames change 1.8–3.2 mean abs per 400 ms) |
| Silence | a stream that stops is named, not glossed over | unit test + real phone (stop → waiting state immediately) |
| Windows host | builds, virtual camera, preview via the same record | windows CI job; **no on-hardware pass** |
| Security review | **open** | see #9 — the repository cannot self-certify |

## P0 — before calling anything released

1. **Independent security assessment of the Wi-Fi pairing** (#9, #4). The existing
   reviews and the surfaces added since them are mapped in
   [reviews/security-coverage.md](reviews/security-coverage.md), including what nobody
   has read yet, and the separate asks in
   [reviews/request-2026-10-21.md](reviews/request-2026-10-21.md) and, for what changed since
   the 2026-10-24 review, [reviews/request-2026-10-25.md](reviews/request-2026-10-25.md). Two things the
   repository cannot self-certify: that the TLS/auth implementation resists review
   by someone who did not write it, and that the negative paths hold under an
   adversary rather than a test.
   *Closes when:* a fresh reviewer's report exists under `docs/reviews/` and each
   finding is either fixed or answered with evidence.
2. **Version and edition reconciliation.** Diagnosed and partly closed 2026-10-25. The numbers were
   never a discrepancy: `0.1.4` and `0.2.0` are the same scheme (`versionCode = major x 1e6 + minor x 1e3
   + patch`, so `1004` and `2000`). Closed: the `v0.2.0` release now carries a **signed
   `Mobile_Webcam-0.2.0-android.apk`** (`versionCode 2000`, `versionName 0.2.0`, `CN=Mobile Webcam`,
   `SHA-256 BA:2A:15:A5…`) with `SHA256SUMS` regenerated and every line re-verified against the published
   bytes. Two facts came out of doing it, and both are recorded in [RELEASE.md](RELEASE.md): the signing
   secrets had never been set (the 0.1.4 APK was signed by hand), and the `publish` job needs a
   self-hosted runner that is **not registered** (`0 runners`), which is why every `release` run in the
   history is `cancelled`.
   *Closes when:* a self-hosted runner with the `mobile-webcam` label is registered (or the workflow is
   repointed to a hosted runner), a tag triggers the workflow end to end, and the field phone — which
   currently runs a **debug-signed** 0.2.0 build, and so needs an uninstall either way — reports the
   released `VERSION`.
3. ~~**Three known lows from the 2026-10-23 review**~~ Closed: the world-readable
   fallback now supplies defaults instead of denying startup, the settings temp name is
   unpredictable, and the largest accepted frame is measured (113 MiB transient on Linux,
   192 MiB on Windows) rather than argued about — all three with tests or a number.
4. ~~**Bounded worker control and real-phone switching acceptance**~~ Closed
   2026-10-24: the switch is accepted on a physical phone through the real capture path —
   `640x480@30` and `1280x720@15` both applied and confirmed by the device
   ([evidence](evidence/2026-10-24-quality-switch-acceptance.md)), driven by a repeatable
   `--quality` argument rather than by clicking a GUI.
   *Closes when:* the #8 matrix passes on hardware and its evidence is attached.

## What other projects offer

[Upstream options](upstream-options.md) surveys crates and projects that could replace
or strengthen what we built — headless egui tests, a PipeWire camera that needs no
kernel module, mutation/fuzz gates, phone screenshot tests — with licence, cost and
the claim each one would let us make.

## P1 — next, each with its closing test

- ~~**USB end to end on this branch.**~~ Closed 2026-10-24: **G0 passes** (the phone
  re-enumerates as `18d1:2d01`) and **G0.5 passes at 1000 round trips** — 1000 sequences
  matched, the 8-byte challenge echoed every time, 0 mismatches, 0 timeouts, 0 disconnects,
  rtt p50 550 µs. Getting there needed three host fixes (stream alignment at connect, a
  `HOST_HELLO` reset for a spent accessory session, and pacing the round trips under the
  phone'"'"'s own PING rate limit — the last one being an instrument that was measuring its own
  violation). Evidence and the ordered story:
  [evidence/2026-10-24-usb-g0.md](evidence/2026-10-24-usb-g0.md). Remaining on this path: the
  same run repeated, and the app-side items in #78 (a per-attach USB approval, and the session
  lifetime that makes `HOST_HELLO` necessary).
- **Two USB stream questions the 2026-10-24 review left open.** Q1: `align_stream`'s "aligned"
  verdict is a guess after 200 ms of silence, and the shipping path discards the verdict. Q2: the
  shipping hello retry cannot realign the stream it retries on. Both are answered in that review's
  disposition, and the third review corrected the first answer: Q1 cannot produce a *malformed* frame
  (the magic and length checks reject one visibly), but a correctly shaped one can still be misparsed
  when alignment was assumed — it constructed such a frame (ID-004) — while the silence verdict is
  deliberate, because an app that has not started sending is silent too — and both close with a device: one that pauses mid-payload past the read timeout, and
  a shipping run whose hello retry succeeds after a misaligned first attempt.
- ~~**The device actually shows the newest UX.**~~ Closed 2026-10-24: an APK built from
  `main` is on the XQ-GE44 (versionCode 2000, dex checked for the new symbols) and the device
  shows `yuu へ配信中（申告名）`, `許可したPC（3台）`, and the pairing key / rotation / erase
  inside Details.
- ~~**A long stream on the phone.**~~ Closed 2026-10-24:
  [ten minutes on the phone with the screen off](evidence/2026-10-24-phone-soak-screen-off.md)
  — 17 851 frames at **30.46 fps**, hardware decode, `submitted == decoded` in every
  sample, screen `Dozing` throughout, thermal +4.5 C battery / +7.8 C CPU. First attempt
  (2026-10-23) reached 6 min 27 s before the phone left the USB bus.
  **What is still missing is energy**, because the phone was on a charger for both runs;
  that needs it unplugged with Wi-Fi as the only link, and the instrument refuses to
  report a drain figure from a charger rather than printing the charger's.
- **Windows on hardware.** The CI job proves it compiles and the unit tests pass; it
  does not prove a DirectShow consumer sees frames.
  *Closes when:* a Windows machine shows the virtual camera in another app and the
  preview renders.
- ~~**Reconnect backoff made visible.**~~ Closed: the line reads
  `スマホを待っています (2回目/8・4秒後に再試行)` / `Waiting for the phone (retry 2 of 8 in 4 s)`,
  with a test pinning the wording including an already-expired deadline.
- ~~**The preview's own cost.**~~ Closed 2026-10-21:
  [2026-10-21-preview-cost.md](evidence/2026-10-21-preview-cost.md) — 320 µs per frame
  and 5 µs to publish, **0.33 % of one core at 10 Hz** for 1280x720 and 1920x1080
  (both scale to 320x180). No period setting is warranted at that price.

## P2 — ideas, not commitments

- Preview fit/crop toggle, and a preview that keeps the last frame dimmed when the
  stream stops (today the last frame stays at full brightness).
- Quality switching shown as progress (taken → applying → confirmed) rather than
  three states.
- ~~A headless CLI mode so scripts and CI can drive a session without a window.~~
  Added 2026-10-22: `mobile-webcam --headless [--transport] [--host] [--seconds]`
  prints every event as JSON and exits 0/1/2 for streamed / not streamed / could not
  start. Its event loop is tested; the streamed path needs a phone.
- Tray/notification thumbnails of the current preview.

## Operational facts worth knowing

- Settings live at `$XDG_CONFIG_HOME/nekomario28/Mobile Webcam.conf` (default
  `~/.config/nekomario28/Mobile Webcam.conf`) — **the same file the classic Qt app
  used**, so a development build overwrites the installed app's pairing. Keep a copy.
- The store **refuses a settings file that other users can read** (`chmod 600`); the
  file holds the access key.
- The PC identity is `Mobile Webcam.identity` beside it; replacing it invalidates
  every phone approval.
- `adb` and the Waydroid emulator can be attached at the same time; always pass
  `-s <serial>`.
- The capture gate (`host-rs/tools/capture-gui-states.sh`) renders every GUI state
  from committed fixtures, including the preview frame, and fails on states that
  become indistinguishable.

## CI cost, measured

The `linux` job was cancelled at its 30-minute limit twice in a row (2026-10-23). The
step timings say why: **the apt dependency install took 17 min 1 s** on that runner, and
the job still had an FFmpeg build, the whole test suite and two release builds ahead of
it. The gate and the release artefacts are now separate jobs and both have a 45-minute
budget — the same correction the windows job needed earlier, for the same reason. A slow
mirror should not be able to cancel the tests.

## Known flake, with a fix

The Windows job's FFmpeg fetch failed three times on 2026-10-23 with
`URLError: Connection timed out` (twice on a PR, once on `main`) and passed on every
rerun. `build_ffmpeg.py::archive` now makes three attempts with backoff, removing any
partial file between them; the URL and sha256 are pinned, so a retry cannot fetch
something else. Verified against an unresolvable host: fails after 3 attempts in 15 s,
no partial file left.

## Gates in place

`host-ci` / `rust-host-ci` (unit, clippy, fmt, Windows target), `android-ci`, the
emulator conformance run, the GUI capture gate, `lint-workflows` (actionlint + zizmor),
`lint-text` (typos + offline link check), a weekly `mutants` run over the preview record (89% of its mutants killed, the rest
equivalent), and a weekly `fuzz` run (the decoders carry
a committed corpus; the stable property tests run per pull request). The host UI itself is now covered by
AccessKit-driven tests, so "clicking this opens that" no longer needs a screenshot.

## Performance

[performance.md](performance.md) is the ledger: per-second work, five-run medians, the
findings worth fixing, and the dismissals that were measured rather than argued. The
bulk — decode, swscale, sink submit — is not measured there, and says so.

## How a claim gets accepted

1. A gate or a test that fails before and passes after (`docs/gates.md`).
2. Evidence captured with the run (`docs/evidence/`), including the numbers.
3. A CHANGELOG line that says what changed and what proved it.

A TODO that produces a screenshot with no gate behind it is a note, not work.
