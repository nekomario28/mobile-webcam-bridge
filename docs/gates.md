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
   the command line is **not** enough (measured: it still fails).
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
