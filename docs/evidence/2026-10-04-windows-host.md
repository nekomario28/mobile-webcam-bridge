# Experimental Windows host — 2026-10-04

Base: `91232b17b443d418cb316b84415e48188bfebba9` (`origin/main`, freshly checked).
Tested implementation commit: `a26ade593158e979ba07e2d3072c0a7e79584a0d`.
Later evidence-only edits do not change the tested implementation.

The shared host adds Winsock transport, a Win32 pipe control reader and Unity
Capture output. Linux keeps its V4L2 CLI; Android keeps its APK and AMB1 protocol.
The Windows GUI initially selects Wi-Fi and probes USB only when USB is selected.
Setup.exe installs the app, runtime libraries and 32/64-bit virtual-camera
filters together, with Start menu shortcuts and an uninstaller. BAT files and
the developer-only video-dump tool are excluded from the distribution.

Windows converts decoded frames directly to RGBA, without the intermediate
YUYV frame. Bottom-up row order and vertical flip share the conversion stride.
Pixel conversion is skipped until a camera consumer opens the receiver; H.264
decoding continues to preserve decoder state.

## Evidence

| Check | Result | Boundary |
| --- | --- | --- |
| Linux Release GUI/host build, `-Werror`, CTest | PASS, 4/4 | Local software checks |
| Linux install manifest, CLI help and GUI process startup | PASS | Local install/loader checks; no new hardware receipt |
| Windows x86_64 MinGW Release GUI/host build, `-Werror` | PASS | Cross compilation |
| Windows test executables under Wine | PASS, 5/5 | Framing, transforms, TCP deadlines/partial reads, Unity IPC |
| Synthetic AMB1 stream to Windows sink | PASS | TCP, H.264 software decode and RGBA shared-memory output |
| RGBA rotation/flip matrix | PASS, 16 combinations | Received quadrant colors, bottom-up order and black sidebars |
| Live stdin 180-degree rotation | PASS | Win32 `CreatePipe` control in the initial port |
| Final NSIS Setup.exe compilation | PASS, warnings treated as errors | NSIS 3.12, final implementation source |
| Final Setup.exe install/update/remove | PASS | Isolated Wine prefix; both registry views and category/COM key removal |
| Installer collision protection, locked-file retry, user-file retention | PASS | Refuses foreign/additional slots; retry succeeds after a held file is released |
| Staged executable identity, dependency closure and camera DLL hashes | PASS | Staged EXEs match built files; clean native Windows installation not tested |

Toolchain: GCC 16.2.1 on Linux, MinGW GCC 16.2.0, CMake 4.4.3, NSIS 3.12,
Wine 11.18. MSYS2 libraries: Qt 6.11.2, FFmpeg 9.0.2, libusb 1.0.30.
Installer and video fixtures ran in separate isolated Wine prefixes.

Independent review identified an uninstall retry defect: early deletion of
camera DLLs could prevent a retry. The DLLs are now deleted after other app
files, and removal no longer loads an already-deleted DLL. Follow-up scoped
reviews established no further actionable defect. Runtime registry/key coverage
was checked separately by the final installer fixture. A broader review reached
its time limit and supplies no completion verdict.

The donor's all-slot unregister failure and the bounded removal decision are
recorded in [`docs/provenance.md`](../provenance.md#windows-installer).

## Remaining acceptance

Actual Windows hardware: NOT RUN. Android USB/AOA WinUSB setup with debugging
off, real Wi-Fi streaming/reconnect, camera enumeration, moving video in target
apps, D3D11VA GPU decode, UAC and clean-machine installation are NOT RUN.
Wine/shared-memory checks do not establish these outcomes. Historical Linux
hardware receipts were not rerun. Start native Windows testing with Wi-Fi and
a real camera consumer; see [`windows/README.md`](../../windows/README.md).

iOS remains unimplemented. Its future browser path is HTTPS Safari capture and
WebRTC, with a new receive path; raw AMB1 TCP is not a browser transport.
[WebKit's media capture documentation](https://webkit.org/blog/7726/announcing-webrtc-and-media-capture/)
describes the HTTPS requirement.
