# What other OSS projects could we take from

A survey run 2026-10-21 over crates.io and GitHub (roughly 40 queries: v4l2loopback
helpers, AOA, DirectShow/Media Foundation virtual cameras, PipeWire cameras, egui
testing, Android screenshot testing, mutation/fuzz/property testing, supply-chain and
Actions analysis, Android measurement tooling). Nothing here has been integrated:
each row says what it would replace, what it would let us prove, and what it costs.

Licence first: this repository is Apache-2.0. Anything GPL/AGPL below is listed as a
**reference to read**, not code to copy.

## Tier 1 — take these

### `egui_kittest` 0.36.2 (MIT OR Apache-2.0) — headless UI tests

Same version as our `eframe`, built on `kittest`/AccessKit, which our build already
enables. It drives the real widget tree: find by label, click, assert.

- **Replaces**: hand-driven X11 input in the capture gate. Measured this session:
  XTest clicks never reached egui (scroll did), so every interaction on the host side
  was verified by eyeballing screenshots; `egui_kittest` would have answered "does
  clicking Details open Details" in a unit test.
- **Lets us prove**: the Details disclosure, the mirror/rotation toggles, the quality
  dropdowns, the refusal guidance — plus snapshot images without Xvfb, openbox or a
  window manager quirk that places windows at negative coordinates.
- **Cost**: one dev-dependency and a harness; the capture gate stays for the
  end-to-end look of a real window.

### `pipewire-vircam` 0.7.1 (Apache-2.0) — a Linux camera without the kernel module

Registers a PipeWire `Video/Source` node and hands us a fillable frame; supports
YUY2/YUYV, RGBA and multiple modes.

- **Replaces**: nothing — it adds a **second Linux sink**. Today every Linux user must
  install `v4l2loopback` (and often `pkexec modprobe`), which is the first thing that
  goes wrong on a new machine.
- **Lets us prove**: a camera that appears with no kernel module, on Wayland, to
  PipeWire-aware consumers (OBS, and browsers behind their PipeWire-camera flags).
- **Cost**: a second output backend behind the existing `Sink` trait, plus honest
  documentation of which apps can see a PipeWire camera today. Keep v4l2loopback as
  the default; this is the "no module" option.

### `cargo-mutants` (MIT) — prove our tests catch bugs

Injects small defects and reports the ones no test notices.

- **Lets us prove**: the claim this repository keeps making — "the test fails before
  and passes after". Two defects this session were found by hand and not by tests
  (the GUI waking only on events, and `updateControls` overruling the Details rule).
  A mutation run over `wire.rs`, `settings.rs` and `preview.rs` would say how much of
  that class is left.
- **Cost**: a nightly job over a subset of files; mutants are slow but the suite is
  small (6 s).

### `arbitrary` + `cargo-fuzz` (MIT OR Apache-2.0) — fuzz the protocol parsers

- **Lets us prove**: that the parsers reachable from the network — `wire::*::decode`,
  `Config::parse`, `Epoch`, `PairingInfo`, the auth message verification — do not
  panic on hostile input. That is a precondition for the independent security review
  (#9), and stronger than the hand-written length checks we have.
- **Cost**: a `fuzz/` crate and one job; seed corpora come from the existing fixtures.

### `Roborazzi` (Apache-2.0) or `Paparazzi` (Apache-2.0) — phone screenshots on the JVM

- **Replaces**: the emulator round trip for layout regressions. Today the phone UI is
  pinned by wiring tests that read source text; a screenshot test would pin what the
  user actually sees (the main screen we just cleaned, the Details group, the approval
  panel).
- **Cost**: a Gradle plugin and golden images; Roborazzi runs on the JVM with
  Robolectric, Paparazzi renders without a device.

### `zizmor` (MIT) — static analysis for the workflows

We run `actionlint`; `zizmor` looks for the security-relevant class: expression
injection, credential handling, unpinned actions, over-broad `permissions`.

- **Lets us prove**: part of what the security review would otherwise have to read by
  hand. Cheap to add to `lint-workflows.yml`.

## Tier 2 — cheap and useful

| Item | Licence | What it does for us |
|---|---|---|
| `zeroize` + `secrecy` | Apache-2.0 OR MIT | wipe the access key and identity from memory; key material is currently plain `String` |
| `subtle` | BSD-3 | constant-time comparison wherever we compare keys/fingerprints with `==` |
| `cargo-vet` | Apache-2.0 | audited-crate policy above `cargo audit`'s advisories; every dependency gets a reviewed justification |
| `typos` | Apache-2.0 | our docs and comments are bilingual; a typo gate is nearly free |
| `lychee` | Apache-2.0 | the docs now cross-link a lot (`roadmap.md`, `gates.md`); broken links rot silently |
| `nextest` | Apache-2.0 | faster, per-test isolation for the Rust suite; matters when mutants and fuzz come in |
| `insta` | Apache-2.0 | snapshot the event/state JSON we already keep in `tests/fixtures` |
| `humantime` | MIT OR Apache-2.0 | "retry in 8 s" formatting for the reconnect backoff item |

## Evaluated and rejected (with the reason)

| Candidate | Licence | Why not |
|---|---|---|
| `virtualcam` crate | **AGPL-3.0** | licence-incompatible; would contaminate the product |
| `webcamoid/akvirtualcamera` | GPL-3.0 | reference only (mature Windows/macOS virtual camera) |
| `Broly1/android-webcam`, `spizganed/Lenny` | GPL-3.0 | direct competitors; read for technique, never copy |
| `linux-usb-gadgets/uvc-gadget` | GPL-2.0 | a different architecture (phone as UVC gadget, needs root and gadget-capable hardware) |
| `v4l` 0.14 (MIT) | MIT | safe V4L2 bindings with `Output` support would replace our generated bindings, but last release 2023-05 and it would rewrite the one path that is already proven on hardware; a spike later, not now |
| `aoa` 0.1.0 (MIT) | MIT | Android Open Accessory, 2020, no repository; our AOA path is tested against a real phone |
| `v4l2loopback` crate 0.1.0 | MIT | creates the device node only; no I/O. Our remaining need is a *second* sink, which `pipewire-vircam` answers |
| CameraX | Apache-2.0 | would remove Camera2 boilerplate but we need exact mode/fps control and per-device quirks; the current path passes the emulator and the real phone |
| `directories` | MIT OR Apache-2.0 | our config path deliberately matches the classic Qt app; a "correct" OS path would break that compatibility |

## Ideas worth copying without code

- **KDE Connect / Syncthing pairing**: both sides see a code and a device name before
  approving, and the device list shows last-seen. We do the second half; the first is
  what our 90-day approval prompt already does.
- **scrcpy**: the "video only, no audio, no root" pipeline and its `--no-playback`
  style flags are the model for a future headless CLI mode.
- **OBS Virtual Camera**: its registration/`OBSVirtualCam` naming is what apps learned
  to look for; worth checking whether our DirectShow friendly name is discoverable by
  the apps that only list known virtual cameras.

## Taken so far

- **`egui_kittest` 0.36.2** — in the tree as a dev-dependency. `host-rs/src/gui/harness_tests.rs`
  drives the real window: a paired host shows its pairing and hides the fields, clicking
  Details reveals them, and a revoked PC is told it was revoked rather than that the key
  is wrong. No display, no window manager, no synthetic clicks.
- **`proptest` + `cargo-fuzz`** — `host-rs/tests/parsers.rs` throws arbitrary bytes at
  every decoder on stable in CI (768 cases × 2 properties per run), and
  `host-rs/fuzz/` runs the same shapes under libFuzzer on a weekly schedule. Measured
  on the first run: **19.6 million executions in 180 s, 513 edges, no crashes**. The
  corpus is committed so a rerun starts where the last one finished.
- **`pipewire-vircam` 0.7.1** — the second Linux camera backend, behind
  `--features pipewire`. No kernel module and no `pkexec modprobe`, at the cost of
  reach: only PipeWire-aware consumers list it today, which the settings UI says
  plainly. It needs **PipeWire 1.4 or newer headers** (`libspa 0.10` uses SPA
  metadata that 1.0.5 does not define), so the CI job checks the version and says it
  skipped rather than failing or going quiet; measured against 1.6.9. The crate is driven by its own timer and *pulls* frames, so the worker's
  push is bridged through one slot; it also requires its main loop on a thread named
  `main` (an assertion inside the `pipewire` crate — measured, not documented).
- **`cargo-mutants`** — a weekly run over the preview record, and the two rounds it
  took to make it useful. First measurement: **185 mutants, 137 caught (77%)**.
  Reading the survivors showed two kinds of weak test: loose bounds (`g < 40` hides a
  ±11 shift in a colour channel) and uniform fixtures (every pixel the same, so index
  arithmetic is invisible). After exact-value assertions and coordinate-carrying
  fixtures: **159 caught, 19 missed (89%)**, the rest equivalent mutants around the
  sequence number. That is the "a test fails before it passes" claim, measured.
- **`zizmor`**, **`typos`**, **`lychee`** — `lint-workflows.yml` and `lint-text.yml`.
  Setup for the first pass: third-party actions are now pinned to commits (GitHub's own
  `actions/*` stay on tags by policy, in `zizmor.yml`), the release job builds without a
  Gradle cache so a pull request cannot poison what gets signed, `_typos.toml` allows the
  kernel's `S_PARM` and NSIS's `StrCpy` (and the deliberately misspelled flag a test
  feeds the parser), and the docs gate runs `lychee --offline` because relative links are
  the ones this repository owns.

## Suggested order

1. `egui_kittest` (unblocks trustworthy host-side UX tests immediately).
2. `zizmor` + `typos` + `lychee` (one small PR, all three are gates).
3. `arbitrary` + fuzz the wire parsers (security review prep).
4. `cargo-mutants` on the three core modules (answers "how much of this is tested").
5. `pipewire-vircam` as a second sink (real user-facing win, largest of these).
