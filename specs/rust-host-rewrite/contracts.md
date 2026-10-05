# Contracts, ownership, and parity

These are design contracts, not implementation evidence. Baseline: v0.1.2 / `03e874d7396f1160f73087d657847760226199bb`.

## Process and state

`SessionController` is the single owner of user intent and visible lifecycle. States: `Idle`, `Connecting(phase)`, `WaitingForCamera`, `WaitingForConsumer` (Windows), `Streaming`, `Stopping`, `Failed(error, action)`. Phase describes USB switch/open, TCP handshake, or output readiness; it is not another state machine in the GUI. The controller owns start/stop intent; worker observations are the source of receiving/output status, not a parallel GUI streaming flag.

The worker sends observations (`Connected`, `VideoConfig`, `Stats`, `TransformApplied`, `Failure`, `Exited`). Decoded/submitted frame counts and consumer readiness are coalesced in Stats; they are not per-frame GUI messages. GUI rendering never derives Streaming merely from a child PID, handshake, or transform acknowledgment. Linux distinguishes decoded frames from completed camera writes; Windows distinguishes decoded frames, frames submitted to IPC, and consumer readiness. With no Windows consumer, show reception plus "waiting for an app to use the camera" rather than claiming frame delivery. Submission/readiness still do not prove a real consumer displayed the frame. Preserve desired versus applied transform values with worker acknowledgment after application.

Each launch has a controller generation and an owned child handle. Messages and stop timers are tied to that generation, preventing an old process from changing or killing the next session. Starting twice is idempotent while starting/running. Stop cancels a pending start and prevents late helper completion from launching a receiver.

The same application executable runs the private worker, with inherited anonymous pipes. Initial config and commands are newline-delimited typed JSON; no video data crosses the GUI pipe. Commands: `SetTransform`, `Stop`; worker configuration is passed once. The private parent/child use the same build, so no version negotiation or RPC framework. Maximum control line 16 KiB; outstanding control messages at most 32, transform changes coalesced, stop prioritized. Drain child output independently of rendering; retain at most 200 log lines of at most 2 KiB. Keep diagnostic logs separate from control observations. Statistics are coalesced at most once per second. Wake/repaint only for input, state changes, or changed statistics.

Normal stop requests graceful termination; after **1 second** terminate/kill that owned worker and reap it without blocking the GUI event loop. Measure cleanup of the output handle/mutex with a deliberately stalled worker. Parent death must also terminate the owned receiver: use a Windows Job Object with kill-on-close and Linux parent-death signaling with a parent identity check; test pipe EOF and forced parent termination independently. Verify OS semantics and failure handling in slices 03/05 rather than trusting Rust Drop.

The Linux privileged USB helper only switches the selected device into AOA and reports the selected physical device; it never receives video or links the GUI/FFmpeg. Connect invokes missing camera integration through the existing script and resumes once after successful setup. When setup also needs the selected USB switch, pass its identity over stdin to the helper under the same authorization. Helper cancellation or setup failure must not restart reception. Its own bounded operation/exit is tested, including authentication cancellation. No reconnect loop, daemon, localhost control server, or worker pool in the initial release.

USB control has only `ListCandidates` and `SwitchSelected`. Windows uses a private short-lived mode of the app; Linux uses the USB-only helper. Listing is always unprivileged. Enumerate when opening/selecting USB, on USB hotplug while that view is active, and at Connect; no repeated enumeration in Wi-Fi mode. SessionController owns these child operations and their generation before starting the receiving worker; only an ACCESS result permits the existing Linux elevated switch.

Candidates are manufacturer-neutral. Recognize an existing accessory by `18d1:2d00/2d01/2d04/2d05`; suggest pre-AOA devices with ADB or standard still-image/MTP interfaces using read-only descriptors, without requiring debugging or a manufacturer list. Metadata is a discovery hint, not proof of AOA support. Keep unclassified non-hub devices in the main picker so charging-only phones are not hidden behind Details. Label devices using available descriptors plus their physical port, without asserting they are Android. Never try request 51 on every USB device to find a phone. Request 51 runs only at Connect on the selected device; a nonzero protocol reply establishes AOA capability. Distinguish access/driver failure, failed capability query, and unplug rather than declaring all failures unsupported. Native Sony and non-Sony checks remain required.

For one suggested candidate, select it automatically. Show the picker when other USB devices are present; require a choice when multiple candidates are suggested. Append a physical-port suffix to product names. There is no Xperia/brand/accessory-mode menu. With no devices, show a short USB-connect prompt; do not launch a receiver. Candidate selection is session-local and revalidated at Connect; a disappearing selection must never silently switch to another attached phone. AOA mode switching is an internal operation, not another user choice.

## UI language

EN/JA is automatic and local. Use the primary OS application/UI locale in the native GUI; Japanese (`ja`, including region/script variants) selects JA, every other or unavailable locale selects EN. Match the current host/Android default rule. Android follows its own app/device locale, not the PC's setting. The HTML sketch uses the first browser preference (`navigator.languages[0]`, then `navigator.language`) and updates on `languagechange`; its collapsed preview override is a fixture only, never a product setting or persisted value.

Keep one small, complete EN/JA copy table for labels, actions, waits, failures and diagnostic summaries. Use stable semantic IDs for controls, options, settings, worker observations and error codes; translated strings must not drive behavior. Keep device product names, addresses, paths, codec/backend names and raw diagnostics unchanged. The GUI maps structured worker failures to short localized actions, with raw details in the bounded log. Language changes must not reconnect, change selection, clear an IP/error, or interrupt Stop. No translation service, runtime download, language wizard or new settings key. Verify native locale retrieval in slice 02 before selecting an API/crate.

## Settings

Keep the existing product namespace, keys and native storage without linking Qt. Keys: `transport`, `lanHost`, `decode`, `output`, `rotation`, `mirror`, `verticalFlip`. Defaults match current code: USB, empty LAN address, Linux `/dev/video10`, decode `auto`, rotation 0, mirror true, vertical flip false. Rotation is 0/90/180/270. There is no new settings file, migration marker or dual writing.

- Linux: `$XDG_CONFIG_HOME/nekomario28/Mobile Webcam.conf`, or the Qt default under `~/.config`.
- Windows: `HKCU\Software\nekomario28\Mobile Webcam`, including the actual Qt-written value types.

Verify the baseline's effective fallback locations and precedence against actual QSettings fixtures (user app, user organization, system app, system organization). Write only the user-app store. Implement the product's scalar keys, not a generic QVariant/QSettings library. Preserve unknown fields and valid existing values. Match Qt UTF-8/escaping/quoting, Windows key case and missing-key behavior through consumer tests.

Only `SettingsStore` writes. Debounce edits by 300 ms and save on action/close, independently of successful connection. Linux writes a sibling temporary file then atomically replaces the file; Windows writes the existing registry values and reports individual write failures. Preserve prior good data on parse/write failure; do not replace an unreadable file with defaults. A failed reload must not silently lose a saved IP. Workers receive an immutable snapshot and never reread/write settings.

Successful save/restore produces no toast or status text. Restored values are visible in their fields. Show a short inline error only when persistence fails, with the underlying diagnostic available in Details. Connection status and settings errors remain separate so one cannot hide the other.

Acceptance includes Qt -> Rust -> Qt -> Rust round trips, Unicode/quoted/escaped hostnames, booleans, unknown fields, missing keys, malformed files, permissions, and failed-connect/restart. The Rust executable must preserve the address without Qt runtime libraries; rollback to v0.1.2 must see subsequent Rust edits.

## AMB1 and transport

Keep the exact external [wire contract](../../protocol/wire-v0.md): little endian, `AMB1`, version 1, 24-byte header, 16 MiB absolute payload cap, sequence and PTS bit patterns, Annex-B video, and existing flags/types. Never reinterpret PTS as a synchronized cross-device clock.

`wire` validates before allocation. Empty HELLO/ACK/IDR payloads, exactly 16-byte VIDEO_CONFIG, and the established 8-byte smoke-test PING/PONG must be covered. Reject truncated/oversized input without allocating unbounded storage. Treat framing sequence as transport-wide: control traffic can interleave with video, so video-only sequence gaps are not a loss detector. The video receiver consumes and ignores message types other than VIDEO_CONFIG/VIDEO_AU; retain unknown type codes rather than rejecting them to make a closed enum.

`Transport` is a local USB/TCP enum with `recv_frame` and `send_frame` behavior; no public plugin API or generic stream adapter hiding USB boundaries. USB send_frame submits **header and non-empty payload as separate bulk-transfer sequences**. Header and payload must never be concatenated into one host-to-phone transfer. Partial payload reads/writes resume at the current offset. Preserve current per-transport deadlines; do not discard a partial payload and continue parsing the same session.

USB header reception uses an endpoint-max-packet buffer clamped to 24–1024 bytes, and discards stale/invalid header transfers until a valid 24-byte header within the existing absolute header deadline. Zero-length packets do not reset the deadline. This existing USB resynchronization is the explicit exception to parser/IO errors terminating a session. Test stale payloads and partial transfers against the baseline. USB has no HELLO handshake: Android accessory input accepts PING and IDR requests only.

TCP host connects to the phone's default **48527** port; HELLO_ACK matches the HELLO sequence and has no payload. Preserve hostname and supported address behavior, TCP_NODELAY, one-client Android behavior, and existing deadlines. A read timeout with no camera does not mean Streaming; distinguish waiting from closed/broken IO using a tested policy. Cancellation is independently guaranteed by worker termination.

USB selection captures the physical bus/port chain and current VID/PID/address before AOA, then follows only that selected physical phone after re-enumeration. Do not persist transient bus/address numbers as an identity across launches. Use requests 51/52/53 and these six exact AOA strings in order: `Android Media Bridge`, `AMB Host`, `Low-latency Android to Linux media bridge`, `0`, `https://github.com/nekomario28/mobile-webcam-bridge`, `amb-g0`. Do not rebrand the transport identity. Verify the resulting accessory interface/endpoints. Normal user access first; Linux elevation only on the existing ACCESS failure. Helpers accept one narrowly typed selected-device operation, not arbitrary commands or paths from a phone. Windows remains subject to libusb-compatible driver binding.

## Video and camera output

`video` owns AVPacket/AVFrame/codec/hardware/scaler lifetimes with RAII; raw pointers and FFmpeg allocation/padding rules stay internal. Any codec callback `opaque` points into stable allocated storage that outlives the codec; the owner cannot move the callback target or release it early. Do not retain a borrowed decoded frame beyond the next decoder call. The libusb context outlives its device handle, including failure cleanup. Decode/convert/output occur in the one worker. No global decoder state, unsafe Send impl, encoded-frame broadcast, or GUI copy of video buffers.

Submit complete Annex-B access units. VIDEO_CONFIG must have the baseline codec/NAL values, positive dimensions/fps, dimensions at most 8192 and at most 33,554,432 pixels. Before starting, ignore access units until VIDEO_CONFIG and a keyframe with CONFIG_INCLUDED. After starting, require strictly increasing PTS even across DISCONTINUITY. DISCONTINUITY flushes the decoder as in the baseline. Active width/height/fps changes and decoder errors terminate the session visibly; do not silently reopen the camera or add automatic recovery to the initial port. First decoded dimensions must match VIDEO_CONFIG. Do not drop arbitrary P-frames and feed their dependents.

Initial path is synchronous read -> decode -> transform -> output, with no additional encoded queue. Enforce at most the protocol's current payload buffer and reusable decoded/converted buffers. If later measurements justify a bounded encoded queue, capacity and discard/IDR recovery must be specified together before it is added. Android already bounds its live queue at 2 access units and owns its sender recovery.

Hardware `auto`: VAAPI then CUDA on Linux, D3D11VA then CUDA on Windows, then software if initialization fails. Explicit selection reports failure. Display the actual active backend. Do not report a configured option as GPU execution evidence.

Reuse conversion buffers and scaler contexts until dimensions/format/transform change. Keep the zero-rotation direct conversion path; apply the same flips and output dimensions as the baseline. For 90/270 degrees, preserve aspect ratio and letterbox into the existing fixed camera dimensions rather than silently swapping consumer dimensions. Preserve YUYV chroma alignment and Windows bottom-up RGBA orientation; pin corners/stride/alpha with asymmetric fixtures. Initial port keeps the existing conversion algorithm; fusion/zero-copy optimizations need a measured follow-up.

Linux output: current V4L2 ioctl/capability/format negotiation, YUYV, default `/dev/video10`, complete frame writes, and external v4l2loopback. Do not create a separate kernel driver.

Windows output: current pinned Unity Capture filter, slot 0, original object names/header layout/events/mutex ordering. Preserve the `Local\MobileWebcam_UnityCaptureProducer` ownership mutex, 3840x2160 bound, RGBA format and stride units (pixels, not bytes). When SendIsReady is false, skip conversion/output while continuing decode so references remain valid; do not treat that no-op as a submitted frame. Preserve the locally hardened 2-second mutex waits and abandoned-mutex release, not unmodified upstream behavior. Choose one implementation of its host IPC after differential tests against `host/third_party/unity_capture/shared.inl`: a direct Rust implementation is the intended target; retain a tiny C ABI bridge only if it avoids a demonstrated compatibility failure, with that decision recorded before release. Filter replacement is out of scope. No consumer does not block reception forever or imply consumer proof.

## Required parity coverage

| Preserved behavior | Owning slice | Evidence required |
|---|---|---|
| Dependency closure / decoding features | 01 | Native build and a deterministic H.264 fixture; recorded feature/import lists |
| Saved IP/settings and rollback | 02 | Qt -> Rust -> Qt round trips and fresh process/restart on both native stores |
| Automatic EN/JA GUI and idle behavior | 02 | Native primary-locale/EN fallback checks, all states/errors, mixed PC/phone locales, IME/keyboard and repaint/idle observations |
| AMB1 bytes and TCP handshake/fragmentation | 03 | Frozen baseline C++/Kotlin fixtures and loopback failure tests |
| H.264 recovery and transforms | 04 | Matched fixture outputs; asymmetric 0/90/180/270 and flip cases |
| Linux camera and GPU paths | 04/07 | Real v4l2loopback and consumer/driver evidence |
| Windows IPC / bounded stop | 05 | Rust vs pinned C++ IPC test, stalls, abandoned mutex, duplicate producer |
| Native Windows camera/GPU/install | 05/07 | Real Windows, consumer, clean install/update/uninstall; Wine supplementary |
| Generic Android selection / USB AOA | 06 | Sony and non-Sony G0/G0.5, 1000 exchanges, USB debugging off, 0/1/multiple candidates, unclassified picker, access failures, unplug and selected-port continuity |
| Actual Streaming, cancellation, disconnect | 07 | Complete GUI-to-worker entry, delayed old events, phone stop/disconnect |
| No regression in live webcam use | 07 | G1/G2 durations and G3 consumers from docs/gates.md, both transports/OS |

Tests may use fixture peers and unregistered Unity Capture slot 40; they do not replace the real device and consumer gates. Do not count a host replay as Android encoder evidence.
