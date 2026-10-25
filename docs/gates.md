# Validation gates

## G0 — AOA capability and transition

Preconditions:

- No ADB dependency in the host path. **USB debugging does not have to be off**: with it on the phone
  exposes two interfaces — adb's (`class 0xff`, subclass `0x42`, protocol `1`) and the accessory's
  (`class 0xff`, subclass `0xff`) — and the host claims the accessory one while adb uses its own.
  Identify the accessory interface from the code that *selects* it (`host-rs/src/usb.rs`), never from a
  descriptor that merely looks plausible. The precondition that *is* real here: the app must already
  have the accessory open, or a host write times out. The numbers behind all of this, including the
  throughput cost of an active instrumentation:
  [evidence/2026-10-25-g0-adb-coexistence.md](evidence/2026-10-25-g0-adb-coexistence.md).
- Data-capable USB cable.
- Mobile Webcam APK already installed by ordinary sideloading.

PASS requires:

1. Host reads non-zero AOA protocol using request 51 while the phone is still
   in its manufacturer USB mode.
2. Host sends zero-terminated manufacturer/model/description/version/URI/serial
   strings using request 52.
3. Host sends request 53.
4. Phone disconnects/re-enumerates as Google VID `0x18d1` with an accessory PID.
5. The target PID is `0x2d00` with USB debugging off and `0x2d01` (accessory + adb) with it on;
   both are accessory modes and the gate accepts either. The probe prints the post-transition
   identity itself, so the transition is read off the device rather than assumed.
6. Android obtains accessory permission and `UsbManager.openAccessory()` succeeds.

Capture exact stdout and `lsusb` before/after as evidence. Prefer selecting the
pre-AOA device explicitly with `--device VID:PID`; do not spray vendor requests
across unrelated USB peripherals when a selector is available.

**Instrument (2026-10-24):** `mobile-webcam --usb-probe [--frames N] [--budget-ms N]` runs
this gate against an attached phone: the AOA transition, the per-PC authorization, then N
synchronous PING/PONG round trips with the sequence and the payload checked on every one,
printing RTT min/avg/p50/p95/max and exiting 0/1/2. `--usb-dump` and `--usb-frames` show
what the phone sends and what the reader makes of it. The first measured run is in
[evidence/2026-10-24-usb-g0.md](evidence/2026-10-24-usb-g0.md): G0 passes, G0.5 passes at
50 round trips and not yet at 1000.

## G0.5 — framed bulk smoke test

PASS requires the Android app to open `UsbAccessory` and exchange **1000**
deterministic AMB `PING`/`PONG` frames over the accessory bulk endpoints:

- sequence matches on every exchange;
- opaque 8-byte challenge is echoed exactly;
- no malformed header / oversized payload / timeout / disconnect;
- host prints same-clock RTT min/avg/p50/p95/max.

Transport boundary: host → Android sends the fixed 24-byte AMB header and any
non-empty payload as separate USB bulk transfers. Android's accessory stream
API drops unread bytes from a USB transfer when a read consumes only part of
that transfer, while the Android parser intentionally reads header then payload.
Do not coalesce both pieces into one host bulk transfer.

No cross-device latency claim is allowed at this gate.

## G1 — 720p30

- H.264, 1280x720, 30 fps.
- 10 continuous minutes.
- decoder corruption = 0.
- unexpected disconnects = 0.
- queue depth remains bounded.

## G2 — 1080p30

- 1920x1080, 30 fps.
- 30 continuous minutes.
- unexpected disconnects = 0.
- visible corruption = 0.
- no monotonic latency growth.

## G3 — Linux consumer compatibility

Validate the virtual camera in at least:

- OBS;
- Chromium/WebRTC camera picker;
- Firefox/WebRTC camera picker;
- Discord (native or browser path available on the test system).

## G4 — 1080p60

Only after G0-G3 are green. Profile camera, encoder, USB throughput, decoder,
pixel conversion, and sink independently before optimization.

## Local JVM gate (the app's unit tests, without CI)

The authoritative route for the Android unit tests is the repository's own
wrapper — `cd android && ./gradlew :app:testDebugUnitTest` — and it is also the
only local route that exercises Gradle's resource linking and its *mockable*
`android.jar`. A cache-only `kotlinc`-based probe is **not** equivalent: it
compiles against a real `android-all` jar, so code that touches Android stubs
compiles there and fails under Gradle. One measured example: a test called
`android.util.Size.getWidth()`, passed in the local probe and failed 1 of 93
cases in Gradle (`Size.getWidth in android.util.Size not mocked`).

Three things stop that command on a machine that has never run it, all worth
checking before concluding the build is broken. Each one was measured here:

1. **The JDK that runs Gradle.** Gradle 8.9 rejects a JDK it does not know:
   `Unsupported class file major version 71` when the default JDK is Java 27.
   Run Gradle itself on a supported JDK:
   `JAVA_HOME=/usr/lib/jvm/java-21-openjdk ./gradlew :app:testDebugUnitTest`.
2. **The Java 17 toolchain the project asks for.** `jvmToolchain(17)` is a
   different thing from the JDK running Gradle, and Gradle will not guess:
   `Cannot find a Java installation … {languageVersion=17 …}`. Install a JDK 17
   and name it in the *user* Gradle properties —
   `org.gradle.java.installations.paths=/path/to/jdk-17` in
   `~/.gradle/gradle.properties`. A `-Dorg.gradle.java.installations.paths=…` on
   the command line is **not** enough (measured: it still fails). On this machine that file
   names `/home/yuu/.gradle/jdks/eclipse_adoptium-17-amd64-linux/jdk-17.0.19+10`, so
   `./gradlew -q javaToolchains` lists *Eclipse Temurin JDK 17.0.19+10* beside the Arch JDKs
   and the tests compile under **17** even though Gradle itself runs on 21. The distinction
   matters when a local run is compared with CI, which runs Gradle on 17 as well.
3. **The SDK.** The project needs `build-tools;34.0.0` and `platforms;android-35`
   with the licences accepted, or Gradle stops with `Failed to install the
   following Android SDK packages as some licences have not been accepted`
   (`sdkmanager --licenses` fixes that). `ANDROID_HOME` must point at that SDK.

With those three in place this machine runs, from the repository wrapper alone:
`./gradlew :app:testDebugUnitTest` → every suite green and **0 failures** (actual Gradle XML, not a
compile-only probe), and `./gradlew :app:assembleDebugAndroidTest` → the instrumentation APK builds.
The test count is deliberately not written down: this section recorded 106, and it measured **152**
on 2026-10-25, so a number here reads as a failure the moment it rots. Run this before trusting any local
compile-only probe; the emulator conformance job remains the only place the real
Android runtime and camera are exercised.

The cache-only `kotlinc` helper that stood in while Gradle could not run here
(`~/Documents/kotlin-jvm-test`) is **superseded and unmaintained**: it was never
shipped in this repository, the three fixes above make the wrapper usable, and
the wrapper is the only route that can be trusted for `android.*` behaviour. The
measurement above — `Size.getWidth()` fine in the probe, unmocked under Gradle —
is the reason to keep using the wrapper rather than the probe.

## Local text gate (typos and offline links, without CI)

`lint-text` has two legs and each is one command on a machine that has the tool:

- **`typos`** — the repository's `_typos.toml` is discovered from the working directory, so
  the command is literally `typos` at the repository root. CI pins `crate-ci/typos` `v1.51.1`
  and the local tool was installed at the same version.
- **`lychee --offline --no-progress '*.md' 'docs/**/*.md' 'specs/**/*.md'`** — the exact
  argument string from `.github/workflows/lint-text.yml`. Offline is deliberate: relative
  links are the ones this repository owns, and an external URL is somebody else's uptime.

Both build from source here in about two minutes
(`cargo install typos-cli --locked` → `typos-cli 1.51.1` in 46 s;
`cargo install lychee --locked` → `lychee 0.24.2` in 1 m 55 s).

Installing them is not the point; **being able to fail is**. Measured 2026-10-25 on this
machine, so neither leg is a green light that proves nothing:

| Injected into a tracked `.md` file | Result |
|---|---|
| a misspelled word | `typos` exits **2**, and names the word, the file and the line |
| a relative markdown link to `docs/does-not-exist-xyz.md` | `lychee` exits **2**, `File not found`, with the file and the line |

With nothing injected, `typos` exits 0 over the whole tree and `lychee` reports 0
errors, so the two exit codes above are what a *finding* looks like rather than what
green looks like.

Both injections are *described* rather than quoted, and that is a measured lesson rather
than style: the first version of this table reproduced them, so documenting the gate
turned the gate red — `typos` flagged the misspelling inside a code span, and `lychee`
resolved the example link and reported the file missing. Reproduce either in a scratch
file when the raw output is wanted.

The link leg has a cheaper stand-in that needs no tool: a relative-link existence check over
the targets of `]()` (skipping `http(s)` and `mailto`). It was used while `lychee` was not
installed here and agrees with it on this tree, but it is not the same target set —
reference-style links, images and `file://` URLs are outside it — so prefer the real tool and
read the stand-in's silence as "no broken relative link", nothing wider.

## Local host gate (the Rust host's tests, clippy and fmt, without CI)

The host legs run from the repository root with the crate's own manifest:

```
cargo test --manifest-path host-rs/Cargo.toml --lib
cargo test --manifest-path host-rs/Cargo.toml --test secure_lan --test parsers
cargo clippy --manifest-path host-rs/Cargo.toml --all-targets --locked -- -D warnings
cargo fmt --manifest-path host-rs/Cargo.toml --check
```

Measured 2026-10-25 on this machine: every leg green - the library reported **160 passed / 0
failed** / 5 ignored, `secure_lan` 5 and `parsers` 2, clippy clean with warnings denied, fmt clean.
The counts carry their date rather than standing as a target: they move (the library was 152 before
`main` gained eight tests), and only the **0 failed** is what a run should check. `--all-targets` matters for the clippy leg - a warning
in a test module is what `-D warnings` turns into a CI failure, and `--lib` alone does not compile
them.

**The Windows target is a CI-only check here.** `cargo check --target x86_64-pc-windows-gnu` stops
on this machine before it reaches any of this repository's code, because the crate wants an FFmpeg
for that target (`Set FFMPEG_DIR …`, `Install FFmpeg via vcpkg`, `Install FFmpeg and ensure
pkg-config can find it`) and the windows job builds its own. So "the host builds for Windows" stays
a claim the workflow makes and this machine cannot, which is worth knowing before concluding a local
failure is a defect.

## What a local run cannot check

- **Every commit in a pull request needs a `Signed-off-by:` trailer matching that commit's own
  author identity.** [CONTRIBUTING.md](../CONTRIBUTING.md) owns the rule and
  `.github/workflows/dco.yml` enforces it, so this note does not restate either - it records the two
  things neither says. First, no local gate fails it, which makes it the one thing a green local run
  does not tell you: measured 2026-10-25, a branch passed the text, host and Android legs and was
  rejected on this alone. Second, `git rebase --signoff <base>` fixes a whole branch at once and
  leaves the **tree hash identical** (measured on that branch), so a verification already made
  against the tree still stands afterwards.

## Running the conformance on a physical phone

The instrumentation refuses a physical device by default, because it rewrites the
app's own authorization store and drives its UI. Two explicit opt-ins satisfy
that, and the pristine-profile check is a second gate that refuses any device
holding a stored approval:

- `-e disposableUi true` — a disposable API 35 emulator, which is what CI uses;
- `-e physicalDevice true` — a physical device the caller has designated.

`android/tools/physical-conformance.sh --serial <serial> --camera|--conformance`
is the way to run either. It clears the app's data first, grants or revokes
`CAMERA` so the mode matches what its assertions expect, and — the part that is
not optional — **restores the device on every exit, including SIGINT and
SIGTERM**.

The UI conformance resizes the display (`wm size 840x1400`), changes the font
scale and switches the app locale, and restores them in a `finally` block. That
block does not run when the process is killed: measured 2026-10-19, an aborted
run left `Override size: 840x1400` on a physical phone until it was reset by hand
(`wm size reset`, `wm density reset`, `settings delete system font_scale`). Hence
the wrapper's trap, and hence running that mode through the wrapper rather than
by hand. The trap was then verified by sending it `SIGTERM` mid-run: it printed
`interrupted; restoring` and the device came back at its physical size with the
font scale, screen timeout and adb forwards clean.

### The single-reader release, on a physical phone

`AccessorySession.closeAndAwait` depends on the platform unblocking a read when the descriptor
closes, and the guard that refusal justifies is only as good as that. The instrument is part of
the instrumentation APK (the emulator has no accessory, so CI cannot run it) and it asserts the
precondition on every iteration instead of assuming it:

```
adb shell am instrument -w -e physicalDevice true -e usbReaderMeasurement true \
  dev.nekomario.amb.test/dev.nekomario.amb.UsbPermissionInstrumentation
```

Order the states from the simplest, and check what the order says about the instrument: with the
endpoint idle a host write times out (measured), with one reader blocked it completes (measured), and
only then does the two-reader state mean anything. A verdict that does not prove it measured
something is not evidence - the release instrument's first version printed `released=true in 0 ms`
twenty times without establishing that the read was blocked, and the frame-owner instrument's first
version reported a consumed frame that was the reader's own proof deadline.

`-e pretendReaderSurvives true` drives the guard's failure path - a reader that outlives its close - on a
healthy endpoint, so the branch that voids the run is exercised rather than merely present. Re-granting
after an app APK install needs the Activity in the foreground when the accessory attaches, because the
system closes the dialog on a requester with no visible Activity.

It needs the accessory attached and the app to hold the USB permission for it, and it installs
nothing: only the androidTest APK is installed, because reinstalling the app APK clears that
grant. **The grant belongs to the uid, not the package name**, so reinstalling cannot provoke the
permission dialog: `adb uninstall -k` plus install reuses the uid and keeps the grant, and `cmd usb` has
no verb to clear it. Installing under a different `applicationIdSuffix` does get a fresh uid - and then
the dialog opens and the system closes it within about a second, because the requester has no visible
Activity (measured 2026-10-25). **Install with `--no-streaming` while the phone is in accessory mode**: the streaming
install path hung twice on 2026-10-25 (a 300 s timeout with no output at all) while the same APK
installed immediately with `--no-streaming`. If an install looks hung, force-stop the app and retry
that way rather than killing the adb server, which a session supervisor restarts anyway. Measured result and its boundary:
[evidence/2026-10-25-usb-reader-release.md](evidence/2026-10-25-usb-reader-release.md).
