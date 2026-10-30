# Mobile Webcam

**English** | [日本語](README.ja.md)

Use an Android phone as a PC webcam over USB or Wi-Fi. USB uses Android Open
Accessory (**no USB debugging / ADB required**); Wi-Fi uses TLS 1.3 with a
pinned phone certificate (Android 10+). English and Japanese follow the system
language, and the PC window shows a live preview so you can frame yourself
before opening another camera app.

## Download and platform setup

[Download](https://github.com/nekomario28/mobile-webcam-bridge/releases/latest)
the Android APK and the desktop app for your OS:

- **Windows:** run **Setup.exe** once to register the virtual camera (requires a
  DX12 graphics driver; USB also requires a libusb-compatible accessory driver).
  The ZIP archive is portable after setup.
- **Linux:** make the AppImage executable and launch it (requires your
  distribution's `v4l2loopback` kernel module, a Vulkan driver/loader, and
  glibc 2.39+; an optional `--features pipewire` source build uses a PipeWire
  video source instead of `v4l2loopback`).

## Quick start

1. Open **Mobile Webcam** on your phone and PC. The phone starts in USB mode.
2. **USB:** plug in the cable and press **Connect** on the PC (a single phone is
   selected automatically). On the phone, allow USB access (tick **always
   allow**), approve this PC for 90 days, then press **Start camera** and allow
   camera access. Approving a PC never starts capture on its own.
3. **Wi-Fi (first time, Android 10+):** after approving the PC over USB in step
   2, press **Send Wi-Fi pairing to PC** on the phone, tap **Wi-Fi** on the
   phone, and press **Switch to Wi-Fi** on the PC (nothing to type). If you
   cannot use a cable, open **Details → Show pairing key** on the phone, verify
   the SHA-256 certificate fingerprint on the phone screen, and enter the phone
   address, fingerprint, and 128-bit key on the PC before pressing **Connect**.
4. Select **Mobile Webcam** in your camera or meeting application.

## Security and compatibility

Current releases (`v0.3.x`) enforce TLS 1.3 with a pinned phone certificate and
128-bit access key on Wi-Fi, plus [per-PC authorization](docs/per-pc-authorization.md)
on both USB and Wi-Fi. Upgrade the Android app and Rust desktop host together;
older builds (`v0.2.0` and earlier, including classic Qt `v0.1.2`) used
unauthenticated plaintext Wi-Fi and do not implement the per-PC gate (do not
expose pre-`v0.3.0` Wi-Fi builds to untrusted networks). Independent security
review of the pairing/authorization protocol and on-hardware Windows consumer
verification remain open; see [security details](docs/security.md),
[migrating to secure Wi-Fi](docs/migration-tls-wifi.md), and the
[roadmap](docs/roadmap.md).

## Contributing and issues

Issues and pull requests are welcome. All contributions require a [Developer Certificate of Origin (DCO)](CONTRIBUTING.md#developer-certificate-of-origin-dco) sign-off (`git commit -s`). For bug reports, mention your phone model, Android version, OS, and transport (USB/Wi-Fi); see [`CONTRIBUTING.md`](CONTRIBUTING.md) for development requirements and hardware gates.

## Documentation and license

[Connection, headless CLI, and setup](host-rs/README.md) ·
[Architecture](docs/architecture.md) ·
[Security](docs/security.md) ·
[Roadmap and evidence](docs/roadmap.md) ·
[Build and verification](specs/rust-host-rewrite/implementation.md) ·
[Classic Qt v0.1.2](docs/classic-qt.md)

Source: [Apache 2.0](LICENSE). Bundled components have their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md),
[FFmpeg source offer](docs/licenses/FFMPEG_SOURCE_OFFER.md), and the notices in
each release package.
