# Connection and setup

Open Mobile Webcam on your phone and PC. Choose the same mode on both:

- **USB:** connect the cable, select the phone on the PC, then press **Connect**.
- **Wi-Fi (development build, Android 10+):** select Wi-Fi on the phone,
  choose **Show Wi-Fi pairing details**, and manually verify the SHA-256
  certificate fingerprint against the phone display. Enter the displayed
  128-bit access key and the phone IP in the Rust desktop client, then press
  **Connect**. Secure Wi-Fi uses TLS 1.3 with certificate pinning. The old
  C++ host cannot connect to the secure-only phone listener.

Press **Start camera** on the phone and allow camera access. If USB access is
declined, select USB again to retry. USB debugging is not required.

## Platform setup

| Platform | Requirement |
| --- | --- |
| Windows | Run Setup.exe once for the virtual camera; DX12 graphics driver. |
| Linux | Install your distribution's v4l2loopback module; Vulkan driver/loader and glibc 2.39 or newer. |

On Linux, **Connect** requests OS authorization when camera integration is
missing, then resumes connecting. Setup does not install kernel modules.
Windows USB also requires a libusb-compatible phone/accessory driver;
Setup.exe installs the virtual camera only.

**Published v0.2.0 releases use plaintext Wi-Fi.** In this unreleased
security branch, Wi-Fi is TLS 1.3-only and requires the correct pinned
certificate and access key; it does not silently fall back to plaintext.
The pairing key is currently stored in regular desktop settings, not an
OS-protected credential vault. Treat the key as sensitive, and rotate it
from the phone if exposed (this disconnects/revokes *all* paired PCs).
Physical Android, Linux and Windows end-to-end acceptance is still pending.
See [security and test boundaries](../docs/security.md).

Rotation, flips and advanced settings are in **Details**.

## Build and verification

The Rust host reuses the Android app, AMB1 protocol, FFmpeg, libusb and Windows
virtual camera. Build entry points are in [tools/](tools/); dependencies are
pinned in Cargo.lock and the build scripts.

[Implementation evidence](../specs/rust-host-rewrite/implementation.md) records
CI and outstanding device checks. Linux/Xperia success was reported by the user;
physical USB on Linux/Windows was not retested in this update. Classic Qt
[v0.1.2](../docs/classic-qt.md) remains available for rollback.
