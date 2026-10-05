# Connection and setup

Open Mobile Webcam on your phone and PC. Choose the same mode on both:

- **USB:** connect the cable, select the phone on the PC, then press **Connect**.
- **Wi-Fi:** use the same network, select Wi-Fi on the phone, enter its displayed
  IP on the PC, then press **Connect**. The address is remembered.

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

Use Wi-Fi on a trusted local network: the current stream is not encrypted or
authenticated. Rotation, flips and advanced settings are in **Details**.

## Build and verification

The Rust host reuses the Android app, AMB1 protocol, FFmpeg, libusb and Windows
virtual camera. Build entry points are in [tools/](tools/); dependencies are
pinned in Cargo.lock and the build scripts.

[Implementation evidence](../specs/rust-host-rewrite/implementation.md) records
CI and outstanding device checks. Linux/Xperia success was reported by the user;
physical USB on Linux/Windows was not retested in this update. Classic Qt
[v0.1.2](../docs/classic-qt.md) remains available for rollback.
