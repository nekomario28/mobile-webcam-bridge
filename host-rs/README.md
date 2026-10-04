# Mobile Webcam — Rust preview

The Rust PC host reuses the existing Android app, USB/Wi-Fi protocol, FFmpeg, libusb and Windows virtual camera. The current production release remains on the C++ host until hardware parity is accepted.

Windows: run Setup.exe, then launch Mobile Webcam. Linux: launch the AppImage; use **Set up camera** once when needed. Linux needs a distribution-provided v4l2loopback module and a Vulkan driver/loader. Windows requires a DX12-capable graphics driver. The GUI follows the OS language (EN/JA), remembers the Wi-Fi address and keeps rotation/flips in Details.

Windows USB needs a libusb-compatible driver for the selected phone/accessory interface. The app does not replace drivers on unrelated devices. Android still requires its camera Start action.

For developers: use Rust 1.98.1 and Cargo.lock. `tools/build_ffmpeg.py` pins the Linux 7.1.5 and Windows 9.0.2 source archives and configures only H.264, the platform hardware backends and avcodec/avutil/swscale. Set FFMPEG_DIR to that prefix, then run `tools/build_linux.sh` or `tools/build_windows.sh` in the matching native/cross toolchain. Build with one job. `tools/package_licenses.py` collects the target runtime licenses, including fonts, from the crate or its exact upstream commit. Linux packaging must run on the Debian 13 builder; `tools/verify_distribution.py` enforces the 60 MiB and glibc 2.39 limits. Windows packaging reuses `windows/package.cmake` with explicit AMB_RUST_EXECUTABLE, AMB_FFMPEG_ROOT and AMB_RUST_LICENSES inputs.

See [implementation evidence](../specs/rust-host-rewrite/implementation.md) for measured checks and outstanding native hardware gates. Wine checks establish supplementary IPC/installer behavior only.
