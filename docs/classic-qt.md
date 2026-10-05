# Classic Qt host — v0.1.2

Use an Android phone as a webcam over USB or local Wi-Fi. Linux outputs a
normal V4L2 camera such as `/dev/video10`. The experimental Windows host outputs
through Unity Capture. Camera applications do not need a Mobile Webcam plugin.

**Languages:** English and Japanese. The app follows the system language and
uses English when no Japanese locale is selected.

**Why Mobile Webcam:** USB camera sharing works with Android USB debugging off.
Desktop settings are remembered between runs. Linux uses an AppImage; Windows
uses one Setup.exe that installs the app and virtual camera.

## What it does

- Streams Android Camera2 video through MediaCodec H.264.
- Supports USB Android Open Accessory and Wi-Fi/LAN.
- Uses VAAPI, CUDA, or software decoding on Linux.
- Corrects the default horizontal mirror and supports live 0°/90°/180°/270°
  rotation plus vertical flip.
- Applies orientation changes without reopening the camera application.
- Ships a Qt 6 GUI and an x86_64 AppImage.

## Quick start

For the experimental Windows build and setup, see
[`windows/README.md`](../windows/README.md). The instructions below cover Linux.

### Use a release

1. Install the APK on Android and download the x86_64 AppImage from the
   project's [Releases](https://github.com/nekomario28/mobile-webcam-bridge/releases).
2. Use a system with glibc 2.39 or newer and install the `v4l2loopback` kernel
   module for your running kernel. The AppImage
   includes libusb, Qt and the decoder. Common package names are:

   ```sh
   # Arch / CachyOS
   sudo pacman -S --needed v4l2loopback-dkms

   # Ubuntu / Debian
   sudo apt update
   sudo apt install v4l2loopback-dkms

   # Fedora: run after enabling RPM Fusion Free
   sudo dnf install v4l2loopback
   ```

   Fedora needs [RPM Fusion Free](https://rpmfusion.org/Configuration) for
   `v4l2loopback`. Other distributions need the equivalent kernel module package and matching kernel headers.

3. Make the AppImage executable and start it:

   ```sh
   chmod +x Mobile_Webcam-0.1.2-x86_64.AppImage
   ./Mobile_Webcam-0.1.2-x86_64.AppImage
   ```

4. If camera access is not ready, press **Set up Linux camera access…** once
   and approve the administrator prompt. The setup creates `/dev/video10`,
   grants device access and arranges module loading at boot. If another
   loopback camera is already usable, select its `/dev/videoX` instead.
5. Follow the [USB](#usb) or [Wi-Fi](#wi-fi) steps below and select **Mobile Webcam**
   in your camera application.

The AppImage includes this setup helper; cloning the repository is unnecessary.
It cannot install a kernel module for your distribution. If other loopback
cameras are already running, adding `/dev/video10` also needs `v4l2loopback-ctl`;
setup leaves those cameras running.

### Build from source

Install the equivalent development packages for your distribution: CMake 3.20+,
a C++20 compiler, pkg-config, libusb-1.0 development files, FFmpeg
`libavcodec`/`libavutil`/`libswscale` development files, Qt 6 Widgets 6.5+,
and `v4l2loopback`. Then run:

```sh
cmake -S host -B build/host -DCMAKE_BUILD_TYPE=Release -DAMB_BUILD_GUI=ON
cmake --build build/host --parallel
ctest --test-dir build/host --output-on-failure
./linux/build-appimage.sh
```

The generated file is `dist/Mobile_Webcam-<version>-x86_64.AppImage`.

## USB

1. Open Mobile Webcam on the phone and connect the USB cable.
2. In the desktop GUI, select **USB**, select the phone and press **Start**.
   Accessory switching and receiver startup happen together.
3. Press **Start camera** on Android. In USB mode it can also be pressed before
   the PC connects; **Cancel** clears the pending start.

The normal streaming target is accessory-only `18d1:2d00`; USB debugging is not
needed for the running camera connection.

For the equivalent terminal flow:

```fish
./build/host/amb-aoa-probe --list
sudo ./build/host/amb-aoa-probe --device 0fce:XXXX --switch
./build/host/amb-v4l2-sink --device /dev/video10 --hw-decode auto
```

## Wi-Fi

1. Select **Wi-Fi** in the Android app. It starts waiting for the PC.
2. Select **Wi-Fi / LAN** on the desktop and enter the displayed phone IP once.
3. Press **Start** on the desktop, then **Start camera** on Android.

Linux and Windows restore the last phone IP and connection mode when reopened.
Android also restores its connection mode. If the router assigns a different
IP, replace the saved address with the one currently shown on the phone.

There is no PIN. The current LAN transport has no authentication or encryption,
so use it on a trusted local network. Only one LAN client is accepted.

The terminal equivalent is:

```fish
./build/host/amb-v4l2-sink \
  --lan 192.168.1.42 \
  --device /dev/video10 \
  --hw-decode auto
```

## Controls

The GUI starts with horizontal mirror correction enabled. Rotation is clockwise;
90° and 270° are fitted into the existing V4L2 size with black sidebars.
Horizontal and vertical flip can be changed independently while streaming.

## Architecture

```text
Android Camera2 -> MediaCodec H.264 -> AMB1 frames -> USB or Wi-Fi
                                      -> decode + transforms -> Linux V4L2
                                                             -> Windows Unity Capture
```

USB and Wi-Fi share the same framed H.264 session, decoder, recovery, transform,
and V4L2 path. The AMB1 wire format and `amb-*` helper names are internal
identifiers retained for compatibility.

## Status

v0.1.2 provides Android, a Linux AppImage and an experimental Windows Setup.exe.
The earlier Linux USB gate used a Sony Xperia XQ-GE44 with USB debugging off and
Discord. This release adds automated checks for settings persistence, USB
startup/cancellation and the shared protocol; fresh phone/camera checks and
Wi-Fi long-running/reconnect checks remain pending.

Windows installation and host tests have been exercised under Wine. Actual
Windows UAC, hardware decoding, USB drivers and consumer applications remain
unverified. The Linux AppImage is built on Debian 13 with generic x86_64
settings; compatibility with older distributions remains unverified. See the [release evidence](../docs/evidence/2026-10-04-release-0.1.2.md).

iOS is a future input path: Safari camera capture over HTTPS and WebRTC is the
intended browser-based route, and requires a new receiver rather than direct
reuse of Android's AMB1-over-TCP stream.

Technical details and test records are in [`docs/architecture.md`](../docs/architecture.md),
[`docs/gates.md`](../docs/gates.md), and [`docs/evidence/`](../docs/evidence/).

## License

Project-owned classic source additionally offers Apache-2.0 under the
[historical grant](licenses/HISTORICAL-LICENSE-GRANT.md), retaining its original
MIT permission. The AppImage contains Qt and FFmpeg under their upstream licenses;
libusb is bundled, while `v4l2loopback` remains a separate kernel dependency. See
[`LICENSE`](../LICENSE), [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md), and
[`linux/APPIMAGE_LICENSES.md`](../linux/APPIMAGE_LICENSES.md).
