# Evidence and design decisions

Captured 2026-10-04. Product baseline: `03e874d7396f1160f73087d657847760226199bb` / v0.1.2, refreshed against origin/main. These are design inputs, not Rust implementation results. Do not cite an upstream main manifest as evidence that a crate version was released.

## Existing code inspected

| Boundary | Baseline source | Decision-changing behavior |
|---|---|---|
| GUI/settings/stop | `host/src/gui_main.cpp`, `host/tests/gui_test.cpp` | Native QSettings namespace/keys; receiver deadline 120s; GUI forced stop after 1s; delayed USB callbacks must not start after Cancel |
| Automatic language | `host/src/gui_main.cpp`, Android `MainActivity.kt` | Host uses `QLocale::system().language()`, Android uses the first app configuration locale; Japanese selects JA, otherwise EN. Rust retains automatic EN/JA without a new preference |
| Framing | `protocol/wire-v0.md`, `host/src/wire.cpp`, Android `Wire.kt`, `VideoWire.kt` | AMB1, LE 24-byte header, 16 MiB cap, Annex-B and existing type/flag values |
| TCP | `host/src/tcp.cpp`, Android `LanServer.kt`, `LanSession.kt` | Port 48527, empty HELLO/ACK with matching sequence; one client |
| AOA/USB | `host/src/aoa.cpp`, `accessory.cpp`, `main.cpp`, Android `AccessorySession.kt` | Exact AOA strings, physical port tracking, separate header/payload transfers, header resync/ZLP handling; no USB HELLO |
| USB discovery/access | `host/src/gui_main.cpp`, `linux/70-mobile-webcam.rules` | Baseline GUI admits Sony VID `0fce` or an existing AOA device and labels the pre-AOA choice Xperia. Linux pre-AOA rule is `0fce:020d`. The Rust design deliberately removes these manufacturer restrictions; the released C++ app is unchanged by this spec |
| Decode/start | `host/src/video_sink_main.cpp`, `hw_decode.cpp` | Config-included keyframe start; PTS strictly increasing; discontinuity flush; active dimension/fps changes and decoder errors abort |
| Transform/output | `host/src/image_transform.cpp`, `yuyv.cpp`, `video_sink_main.cpp` | Fixed dimensions, letterboxing, YUYV alignment and bottom-up RGBA; no Windows conversion without a consumer |
| Windows IPC | `host/third_party/unity_capture/shared.inl`, `host/tests/unity_capture_test.cpp` | Slot 0 names/ABI, pixel stride, producer mutex, locally hardened 2s waits; slot 40 test is not native camera proof |
| Android lifecycle | `MainActivity.kt`, `FramedOutbound.kt` | Disconnect stops the camera; compressed sender queue is already bounded at 2 with IDR recovery |
| Packaging | `windows/installer.nsi`, `package.cmake`, `linux/install-host-integration.sh`, `docs/gates.md` | Own-file/registration boundaries, 32/64-bit filter, Linux kernel module external, existing hardware acceptance gates |

Unity Capture remains pinned to `3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2`. Preserve the product's local hardening rather than copying unmodified upstream code.

## Distribution measurements

Measurements used the existing v0.1.2 staging/AppDir, excluding symlink duplication. MiB means 1,048,576 bytes. Groups overlap unless marked exclusive.

| Measure | Windows x86_64 | Linux x86_64 |
|---|---:|---:|
| Expanded package | 192,976,742 B / 184.0 MiB | 191,632,856 B / 182.8 MiB |
| Shipped binaries | 95 DLLs | 140 ELF files including executables |
| Qt-exclusive libraries | 63.2 MiB | 69.4 MiB |
| FFmpeg-exclusive, non-core candidates | 84.5 MiB | 84.6 MiB |
| Candidate removal before replacement cost | 147.8 MiB | 154.0 MiB |
| Binary package inventory | 67 | 111 |

Windows Setup.exe: 51,438,312 bytes; Linux AppImage: 68,975,096 bytes; existing APK: 666,549 bytes. Compression prevents subtracting expanded candidates directly from installer size. This audit establishes where size is concentrated; it does not prove a minimal FFmpeg build, final Rust size, or fewer build-time crates.

Baseline build environments: Windows MSYS2 MINGW64, Qt 6.11.2 / FFmpeg 9.0.2 / libusb 1.0.30; Linux Debian 13, Qt 6.8.2 / FFmpeg 7.1.5 / libusb 1.0.28. Baseline Linux dependency scan indicated glibc 2.39 and x86-64 baseline; derive and recheck these from the actual packaged ELF in slice 01.

## Primary references checked

- [eframe's native architecture](https://github.com/emilk/egui/blob/main/crates/eframe/README.md), [feature manifest](https://github.com/emilk/egui/blob/main/crates/eframe/Cargo.toml), [egui behavior](https://github.com/emilk/egui). Native GUI is a candidate; upstream main's system-font feature must be checked in a released version. One rendering backend and event-driven repaint are the proposed configuration.
- [ffmpeg-next manifest](https://github.com/zmwangx/rust-ffmpeg/blob/master/Cargo.toml), [ffmpeg-sys manifest](https://github.com/zmwangx/rust-ffmpeg-sys/blob/master/Cargo.toml), [FFmpeg decoder configuration](https://ffmpeg.org/ffmpeg-codecs.html#Decoders). Narrow wrapper features do not narrow prebuilt FFmpeg libraries. Native configure/import closure is a separate gate; exact hardware flags and license/source obligations follow the selected build.
- [rusb](https://github.com/a1ien/rusb), [nusb backend source](https://github.com/kevinmehall/nusb/blob/main/src/lib.rs). nusb uses WinUSB/usbfs; changing bindings does not remove device-driver/access requirements. Retain libusb initially because the measured native library is small compared with the compatibility work.
- [AOA 1.0 detection and handshake](https://source.android.com/docs/core/interaction/accessories/aoa), [AOA 2.0 accessory PIDs](https://source.android.com/docs/core/interaction/accessories/aoa2). Pre-AOA manufacturer IDs do not establish support; request 51 returns a nonzero protocol version on supporting devices. Design inference: use metadata only to shortlist phones, then query the selected device, with a manual fallback for unclassified devices. No universal Android descriptor or all-model support is assumed. Slice 06 must verify the exact metadata rule and native access paths.
- [QSettings fallback](https://doc.qt.io/qt-6/qsettings.html#fallback-mechanism), [native locations](https://doc.qt.io/qt-6/qsettings.html#locations-where-application-settings-are-stored). Read native product settings without Qt, write the existing user-app location, and verify serialization/precedence with Qt-written fixtures.
- [Rust FFI guidance](https://doc.rust-lang.org/nomicon/ffi.html), [Microsoft Rust bindings](https://github.com/microsoft/windows-rs). Rust safety does not validate an external ABI or remove explicit lifetime/resource constraints.
- [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [Linux parent-death signal semantics](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html). Job kill-on-close depends on final-handle ownership. Linux semantics concern the creating thread and have a registration race/credential reset boundary. Initialize and verify the unprivileged child explicitly; do not assume an elevated helper inherits that guarantee.
- [Android NSD](https://developer.android.com/develop/connectivity/wifi/use-nsd), [Safari WebRTC/media capture](https://webkit.org/blog/7726/announcing-webrtc-and-media-capture/). These establish candidate later mechanisms. Local trusted HTTPS, pairing, receiver choice and lifecycle remain unresolved.
- [Browser language preferences](https://developer.mozilla.org/en-US/docs/Web/API/Navigator/languages), [languagechange](https://developer.mozilla.org/en-US/docs/Web/API/Window/languagechange_event). Browser preferences are ordered; the sketch uses the first preference for the existing Japanese-or-English rule and rerenders translated copy on language change. Its preview override does not change browser settings or persist a product language preference. Native Rust locale retrieval remains a slice 02 check.

## Three independent drafts

Three read-only `codex review` sessions inspected the same baseline and shared brief independently; all exited 0. Each was prohibited from reading the other drafts or this spec. No alternative vendor CLI or native subagent tool was available; these are three independent runs of one vendor, not vendor diversity. Review outputs are research inputs, not independent runtime validation.

| Draft | Perspective / session | Main proposal |
|---|---|---|
| A | Fewest slices, `01a106a7-f215-70d3-bc05-f88b966ad4af` | Three broad slices, native settings, early Rust GUI calling old helpers |
| B | Compatibility risk, `01a106a7-f215-7bb0-bfb7-9680593330a1` | Seven staged boundaries, actual native settings fixtures, Windows driver/upgrade traps |
| C | Ownership, `01a106a7-f215-7b12-86f2-91be71ea5edb` | Six seams, single resource owners, parent-death cleanup, FFI callback lifetime |

All supported worker separation, FFmpeg/libusb continuity, a minimal GUI and no service/plugin/WebView/codec rewrite. They did not establish egui or final binary size through experiments.

## Resolution of disagreements

1. Use seven verification slices: A's receiver slice combined USB, codec and two OS outputs, making a failure hard to isolate. B/C's seams preserve smaller, independently reviewable results. They remain one Cargo package rather than seven subsystems/crates.
2. Use the same native settings store. This replaces the initial JSON-import idea and allows new edits to survive rollback. Product-key serialization is verified by Qt -> Rust -> Qt consumers; no generic QSettings reimplementation.
3. Keep C++ usable in its existing build until cutover, but do not add an interim GUI-to-C++ protocol adapter. A UI-only state driver and headless fixture entry provide early checkpoints without a second production interface to maintain.
4. Use typed bounded private worker messages rather than parsing legacy human logs. No version negotiation, RPC layer, event database or daemon. Generation ownership addresses the concrete stale-child cancellation failure.
5. Preserve existing codec error/config/PTS behavior. An early proposed automatic decoder recovery/reopening was rejected after direct code inspection. Android already owns sender recovery; host queue/retry machinery needs new evidence before adoption.
6. Linux's privileged AOA operation gets a USB-only helper. Calling the GUI/codec-linked application under elevation would widen the current privilege/dependency boundary. The receiving worker remains the same unprivileged executable as the GUI.

## Reopen conditions and current boundary

- GUI choice reopens on a demonstrated native Japanese-input/accessibility/startup/size/idle failure in 02.
- Binding choice reopens if released crates cannot link the minimal baseline FFmpeg/GPU paths in 01; no speculative fork.
- Windows IPC bridge is permitted only for a recorded differential failure; one implementation remains in production.
- A transport/queue/zero-copy optimization requires a measured parity bottleneck. It is not a prerequisite to the rewrite.
- The 60 MiB objective is provisional, not a passing result. Record the combined costs early; do not weaken a gate for a green label.

Scope audit: design and sketch only; the user requested publishing the accumulated work on `rust-host-design`. There is no new release, main change, Android rewrite or speculative iOS/Wi-Fi discovery implementation. User concerns about BAT, repeated IP entry, avoidable processing, generic Android support, automatic EN/JA and Linux/Android setup were carried into the concrete contracts. Rust code, minimal FFmpeg, native GUI, fresh phone/GPU/consumer acceptance and final package size are all NOT RUN.

Design artifact checks: all local Markdown links resolved, no trailing whitespace, and the HTML script passed `node --check`. The [sketch interaction observations and visual boundary](ui-review.md) record the browser checks and the unavailable independent image critique. None of these checks accept an implementation slice.
