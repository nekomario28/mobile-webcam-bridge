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
   has read yet, and the separate ask in
   [reviews/request-2026-10-21.md](reviews/request-2026-10-21.md). Two things the
   repository cannot self-certify: that the TLS/auth implementation resists review
   by someone who did not write it, and that the negative paths hold under an
   adversary rather than a test.
   *Closes when:* a fresh reviewer's report exists under `docs/reviews/` and each
   finding is either fixed or answered with evidence.
2. **Version and edition reconciliation.** The phone in the field reports
   `versionCode 1004 / versionName 0.1.4`, the repository's release is `0.2.0`, and
   `VERSION` drives every artifact (`docs/RELEASE.md`). Until these agree, "which
   build am I talking to" is a guess.
   *Closes when:* `docs/RELEASE.md` names the edition and the field build reports the
   same string, and a release cut from `main` produces APK + Linux + Windows artifacts
   that install over the field build.
3. ~~**Three known lows from the 2026-10-23 review**~~ Closed: the world-readable
   fallback now supplies defaults instead of denying startup, the settings temp name is
   unpredictable, and the largest accepted frame is measured (113 MiB transient on Linux,
   192 MiB on Windows) rather than argued about — all three with tests or a number.
4. **Bounded worker control and real-phone switching acceptance** (#10, #8). The
   staged-apply state machine and the bounded control queue need the real-phone
   matrix from #8 run against them, not the synthetic peer.
   *Closes when:* the #8 matrix passes on hardware and its evidence is attached.

## What other projects offer

[Upstream options](upstream-options.md) surveys crates and projects that could replace
or strengthen what we built — headless egui tests, a PipeWire camera that needs no
kernel module, mutation/fuzz gates, phone screenshot tests — with licence, cost and
the claim each one would let us make.

## P1 — next, each with its closing test

- **USB end to end on this branch.** The AOA path predates the auth work and has not
  been re-run since. Needs USB debugging off, so adb cannot drive it.
  *Closes when:* gates G0, G0.5 and G1 pass with captured `lsusb` and stdout.
- **The device actually shows the newest UX.** The installed build predates the
  receiver name, the approved-PC count and the key auto-hide.
  *Closes when:* an APK built from `main` is on the phone and a screenshot shows
  `yuu へ配信中（申告名）` and `許可したPC（1台）`.
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
- **Reconnect backoff made visible.** "Waiting (retrying)" exists; how many attempts
  are left and when the next one lands does not.
  *Closes when:* the state line names the attempt ("retry 2 of 5 in 8 s").
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
