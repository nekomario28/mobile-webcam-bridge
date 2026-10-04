# Experimental Windows host — 2026-10-04

Base: `91232b17b443d418cb316b84415e48188bfebba9` (`origin/main`, freshly checked).
Implementation source tree tested:
`31e7f689d50499605fb45be9cd739e4f88cbe0de` (before evidence/provenance-only edits).
Work branch: `windows-host`. No remote publication was performed.

The shared host adds Winsock transport, a Win32 pipe control reader, a Unity
Capture output and platform-specific Qt launch paths. Linux retains the same
V4L2 CLI and Android retains its AMB1 protocol/APK. D3D11VA is the Windows auto
decode candidate; its real GPU behavior remains unverified.

## Evidence

| Check | Result | Boundary |
| --- | --- | --- |
| Linux Release GUI/host build, `-Werror`, CTest | PASS, 4/4 | Local software checks |
| Windows x86_64 MinGW Release GUI/host build, `-Werror` | PASS | Cross compilation |
| Windows test executables under Wine | PASS, 5/5 | Framing, transforms, TCP timeout/partial reads, Unity IPC |
| Windows packaged CLI help and GUI startup | PASS | Wine loader/process startup; no visual usability claim |
| Synthetic AMB1 stream to Windows sink | PASS | TCP, H.264 software decode, bottom-up RGBA shared-memory output |
| Live stdin 180-degree rotation | PASS | Win32 `CreatePipe` control; received red/blue regions swap |
| ZIP dependency closure, filter hashes, archive structure | PASS | Cross packaging; clean native Windows installation not tested |
| Focused independent `codex review` | No concrete findings | Read-only review of packaging, sockets/pipes and Unity ABI |

Toolchain: native GCC 16.2.1; MinGW GCC 16.2.0; CMake 4.4.3; Wine 11.18.
MSYS2 library versions: Qt 6.11.2, FFmpeg 9.0.2, libusb 1.0.30.
The synthetic fixture used software decoding and an isolated Wine prefix.

The initial independent review reached its time limit without a final verdict;
a subsequent focused review completed. Its sandbox could not bind the Linux
loopback test socket; the independently run unsandboxed Linux/Wine tests passed.
The initial live-control fixture inherited a Unix pipe into Wine, where
`PeekNamedPipe` returned error 50 (`ERROR_NOT_SUPPORTED`). A Win32 `CreatePipe`
launcher, matching the Windows GUI's pipe mechanism, passed using the same sink.
No production workaround was added for that fixture limitation.

## Remaining acceptance

Actual Windows hardware: NOT RUN. Android USB/AOA WinUSB setup with debugging
off, real Wi-Fi streaming/reconnect, camera enumeration, moving video in Discord
or other target apps, D3D11VA GPU decode and clean-machine installation are NOT RUN.
Wine/shared-memory results do not establish these outcomes. Existing Linux
hardware receipts remain historical and were not rerun for this change.

The next useful check is the experimental ZIP on Windows with the current
Android APK, starting with Wi-Fi and a real camera consumer. Windows setup and
native build instructions live in [`windows/README.md`](../../windows/README.md).

iOS remains unimplemented. The future browser path is HTTPS Safari capture plus
WebRTC, with a new receive path; raw AMB1 TCP is not a browser transport. WebKit's
[media capture documentation](https://webkit.org/blog/7726/announcing-webrtc-and-media-capture/)
is the source for the HTTPS camera requirement.
