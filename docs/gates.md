# Validation gates

## G0 — AOA capability and transition

Preconditions:

- USB debugging OFF.
- No ADB dependency in the host path.
- Data-capable USB cable.
- Mobile Webcam APK already installed by ordinary sideloading.

PASS requires:

1. Host reads non-zero AOA protocol using request 51 while the phone is still
   in its manufacturer USB mode.
2. Host sends zero-terminated manufacturer/model/description/version/URI/serial
   strings using request 52.
3. Host sends request 53.
4. Phone disconnects/re-enumerates as Google VID `0x18d1` with an accessory PID.
5. With USB debugging OFF, target PID is normally `0x2d00`.
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
`./gradlew :app:testDebugUnitTest` → **12 suites, 106 tests, 0 failures** (actual
Gradle XML, not a compile-only probe), and `./gradlew :app:assembleDebugAndroidTest`
→ the instrumentation APK builds. Run this before trusting any local
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

It needs the accessory attached and the app to hold the USB permission for it, and it installs
nothing: only the androidTest APK is installed, because reinstalling the app APK clears that
grant. Measured result and its boundary:
[evidence/2026-10-25-usb-reader-release.md](evidence/2026-10-25-usb-reader-release.md).
