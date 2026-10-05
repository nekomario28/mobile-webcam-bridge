# Mobile Webcam — Rust preview

The Rust PC host reuses the existing Android app, USB/Wi-Fi protocol, FFmpeg, libusb and Windows virtual camera. The current production release remains on the C++ host until hardware parity is accepted.

Install the Android APK supplied with the beta and open Mobile Webcam on your phone and PC. Choose the same mode on both:

- USB: connect the cable, select your phone on the PC, then press **Connect**.
- Wi-Fi: use the same network, enter the IP shown on your phone on the PC, then press **Connect**. The old app's `IP:port` display can also be pasted.

On your phone, press **Start camera** and allow camera access.

Windows: run Setup.exe before launching Mobile Webcam. Linux: launch the AppImage; **Connect** handles missing camera integration through OS authorization and resumes after setup. Linux needs a distribution-provided v4l2loopback module and a Vulkan driver/loader. Windows requires a DX12-capable graphics driver. EN/JA follows the OS language. The Wi-Fi address is remembered; rotation/flips remain in Details.

USB is experimental. Connection failures have been reported on Linux and Windows; the discovery/setup fixes have not been retested with a phone. USB debugging is not required: MTP devices are suggested and unclassified devices remain selectable. Windows USB still needs a libusb-compatible driver for the selected phone/accessory interface; Setup.exe currently installs the virtual camera, not that USB driver. Android requires camera/accessory permission and its camera Start action.

For developers: use Rust 1.98.1 and Cargo.lock. `tools/build_ffmpeg.py` pins the Linux 7.1.5 and Windows 9.0.2 source archives and configures only H.264, the platform hardware backends and avcodec/avutil/swscale. Set FFMPEG_DIR to that prefix, then run `tools/build_linux.sh` or `tools/build_windows.sh` in the matching native/cross toolchain. Build with one job. `tools/package_licenses.py` collects the target runtime licenses, including fonts, from the crate or its exact upstream commit. Linux packaging must run on the Debian 13 builder; `tools/verify_distribution.py` enforces the 60 MiB and glibc 2.39 limits. Windows packaging reuses `windows/package.cmake` with explicit AMB_RUST_EXECUTABLE, AMB_FFMPEG_ROOT and AMB_RUST_LICENSES inputs.

See [implementation evidence](../specs/rust-host-rewrite/implementation.md) for measured checks and outstanding native hardware gates. Wine checks establish supplementary IPC/installer behavior only.
