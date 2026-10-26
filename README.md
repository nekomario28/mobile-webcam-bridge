# Mobile Webcam

Use an Android phone as a webcam over USB or Wi-Fi. English/Japanese follows
the system language. Your PC remembers the phone address and settings.

**Wi-Fi security:** the current source implementation is moving to TLS 1.3,
with a pinned phone certificate and 128-bit access key. The latest published
release may still use **unauthenticated plaintext Wi-Fi**; do not expose it to
untrusted networks. See [security details](docs/security.md) and
[migrating to secure Wi-Fi](docs/migration-tls-wifi.md) before upgrading.
Integration development builds also require [per-PC approval](docs/per-pc-authorization.md)
on both transports. Upgrade Android and the Rust host together; older hosts,
including classic Qt on USB, do not implement that gate. This auth work remains
unreviewed and is not a release-readiness claim.

[Download](https://github.com/nekomario28/mobile-webcam-bridge/releases/latest)
the Android APK and the desktop app for your OS.

1. Open Mobile Webcam on your phone and PC. The phone starts on USB.
2. **USB:** plug in the cable. On the PC, press **Connect** (a single phone is
   already selected). On the phone, allow USB access and tick **always allow**,
   allow this PC for 90 days, then press **Start camera** and allow the camera.
   Approval alone never starts capture.
3. **Wi-Fi, first time:** finish step 2 until the PC is allowed, then press
   **Send Wi-Fi pairing to PC** on the phone (nothing to type). On the phone tap
   **Wi-Fi**. On the PC press **Switch to Wi-Fi**, allow the PC if it asks, then
   **Start camera**. A PC that cannot use the cable can still open **Details →
   Show pairing key** and type the address, the certificate SHA-256, and the
   128-bit key. Verify the fingerprint on the phone before **Connect**.
4. Select **Mobile Webcam** in your camera application. The PC window previews
   the picture before that app is opened.

While it streams, the PC window shows a preview of what the camera delivers, so
you can frame yourself without opening another app. The preview also works when
nothing has attached to the virtual camera yet, and updates a few times a second
independently of the window.

Windows: run **Setup.exe** once to install the virtual camera. The ZIP is
portable after setup. Linux: make the AppImage executable and launch it.

[Connection and setup](host-rs/README.md) ·
[Roadmap and TODO](docs/roadmap.md) ·
[Build and verification](specs/rust-host-rewrite/implementation.md) ·
[Classic Qt v0.1.2](docs/classic-qt.md)

Source: [Apache 2.0](LICENSE). Bundled components have their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md) and the licenses in each package.
