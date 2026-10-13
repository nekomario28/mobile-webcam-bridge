# Mobile Webcam

Use an Android phone as a webcam over USB or Wi-Fi. English/Japanese follows
the system language. Your PC remembers the phone address and settings.

**Wi-Fi security:** the current source implementation is moving to TLS 1.3,
with a pinned phone certificate and 128-bit access key. The latest published
release may still use **unauthenticated plaintext Wi-Fi**; do not expose it to
untrusted networks. See [security details](docs/security.md) and
[migrating to secure Wi-Fi](docs/migration-tls-wifi.md) before upgrading.

[Download](https://github.com/nekomario28/mobile-webcam-bridge/releases/latest)
the Android APK and the desktop app for your OS.

1. Open Mobile Webcam on your phone and PC.
2. Choose USB or Wi-Fi on both. For a TLS-capable development build, tap
   **Show Wi-Fi pairing details** on the phone and enter its certificate SHA-256
   fingerprint and 128-bit access key in the desktop Wi-Fi fields. Verify the
   fingerprint directly on the phone before pressing **Connect**.
3. Press **Start camera** on the phone and allow camera access.
4. Select **Mobile Webcam** in your camera application.

Windows: run **Setup.exe** once to install the virtual camera. The ZIP is
portable after setup. Linux: make the AppImage executable and launch it.

[Connection and setup](host-rs/README.md) ·
[Build and verification](specs/rust-host-rewrite/implementation.md) ·
[Classic Qt v0.1.2](docs/classic-qt.md)

Source: [Apache 2.0](LICENSE). Bundled components have their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md) and the licenses in each package.
