# Rust implementation evidence

The PC host is implemented in `host-rs/` on `rust-host`. The C++ host and production release selector remain the rollback baseline. Software implementation does not close the hardware acceptance slices.

ADOPT: Android APK/Kotlin, AMB1/AOA contract, FFmpeg, libusb, pinned Unity Capture filter, installer ownership/uninstall logic, Linux integration assets and existing stream/transform fixtures. ADAPT: framing/deadlines, selected physical USB port, decoder and output ownership, transforms, QSettings scalar storage, and application/worker supervision. The C++ donor remains until parity acceptance.

## Measured checks

- Rust/Cargo 1.98.1, exact crates in Cargo.lock; Linux native build on Debian 13 and Windows GNU cross-build. Native Rust CI is added separately; its result must be checked for the pushed revision.
- Minimal FFmpeg 7.1.5 (Linux) and 9.0.2 (Windows): actual runtime enumeration contains H.264 only, no encoders. Only avcodec/avutil/swscale are built. Linux VAAPI/NVDEC and Windows D3D11VA/NVDEC are configured; GPU execution remains NOT RUN. Three core libraries total 3,326,688 / 5,267,498 bytes respectively.
- Linux core tests and integration tests pass: frozen AMB1/high-bit unsigned PTS, fragmented TCP HELLO/ACK, absolute deadline, mid-payload EOF, bounded typed controls, damaged settings protection, AOAv2 PID selection, real H.264 decode, 16 transform combinations, YUYV chroma/short-write protection, helper cancellation, and stale video status.
- Real application worker: repeated Start/Stop while a peer is stalled, then abrupt parent exit. Linux PDEATHSIG and Windows kill-on-close Job ownership are exercised separately. Windows execution through isolated Wine is supplementary evidence.
- Qt -> Rust -> Qt saved settings pass with Unicode, variable-width control escapes, unknown byte arrays and 100 saves. Fixtures are isolated from production settings. Windows uses native registry APIs under isolated Wine; Linux uses the native INI path format.
- Packaged Windows executable -> synthetic TCP H.264 -> fresh pinned Unity IPC receiver, including live rotation: PASS under Wine. These callbacks include old frames and do not establish distinct-frame throughput or a DirectShow consumer gate.
- Reused Windows Setup.exe installation, relocation/update, collision rejection, locked-file retry, uninstall and user-file retention: PASS under isolated Wine. Native Windows installation remains NOT RUN.
- Native Linux GUI captures use Xvfb and the Vulkan software adapter. EN/JA capture environments must set LANGUAGE as well as LC_ALL because the Unix locale provider prioritizes LANGUAGE. Japanese IME, accessibility and native Windows GUI remain NOT RUN.

## Runtime and UX

One app executable owns its GUI and a private receiving worker. Workers exist only during a connection; Linux additionally ships a small USB-only privilege helper. Typed commands are bounded, transforms coalesce, Stop has a one-second forced termination deadline, and reaping does not block the GUI. A paused video stream returns to waiting. Wi-Fi mode parks USB discovery; unchanged USB lists do not repaint the GUI. Frames are decoded while the Windows consumer is absent, with pixel conversion/submission skipped when demand stops.

EN/JA follows the OS locale, the existing Wi-Fi address/settings survive restart and rollback, and successful saves are silent. USB selection is manufacturer-neutral. AOA capability requests target only the explicitly selected physical device, with exact identity checks before a switch and same-port continuity only for an owned switch. Linux installs the existing one-time setup assets and the narrow helper; it does not run a root receiver. Windows retains the existing virtual camera and Setup.exe. No BAT launch path or persistent daemon is added.

Target runtime crate graphs contain 229 Linux / 148 Windows packages; Cargo.lock also records other targets/build tools. This is a build dependency count, not shipped dynamic libraries. Both packaging paths reject Qt, unrelated FFmpeg libraries, expanded size above 60 MiB and Linux glibc requirements above 2.39. Exact package receipts accompany each built preview.

## Remaining acceptance

Run the frozen G0–G3 phone/USB/720p/1080p/real-consumer matrix, Linux privilege/setup behavior and native Windows consumer/installer/GUI checks. Measure actual VAAPI/D3D11VA/CUDA and software fallback, native IME/accessibility, and baseline CPU/GPU/RSS/latency. No native phone or GPU gate is accepted from Wine, cross-compilation, synthetic fixtures or software-rendered screenshots. Keep the production C++ release selected until those checks pass. iOS/WebRTC and Wi-Fi discovery remain subsequent features.
