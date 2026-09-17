# Mobile Webcam

![Mobile Webcam Linux GUI in English with an Xperia connected over USB](docs/screenshots/mobile-webcam-linux-gui-en.png)

*Current AppImage screenshot: Xperia connected through USB AOA.*

Use an Android phone as a Linux webcam over USB or local Wi-Fi. The Linux side
outputs a normal V4L2 camera such as `/dev/video10`, so camera applications do
not need a Mobile Webcam plugin.

**Languages:** English and Japanese. The app follows the system language and
uses English when no Japanese locale is selected.

## What it does

- Streams Android Camera2 video through MediaCodec H.264.
- Supports USB Android Open Accessory and Wi-Fi/LAN.
- Uses VAAPI, CUDA, or software decoding on Linux.
- Corrects the default horizontal mirror and supports live 0°/90°/180°/270°
  rotation plus vertical flip.
- Applies orientation changes without reopening the camera application.
- Ships a Qt 6 GUI and an x86_64 AppImage.

## Quick start

1. Download the signed APK and Linux AppImage from the project's
   [Releases](https://github.com/nekomario28/mobile-webcam-bridge/releases), or
   build them from source.
2. Install the Linux packages for your distribution below.
3. If `/dev/video10` does not exist, or the Android accessory cannot be
   opened without sudo, install the USB rules and virtual-camera integration
   once from this checkout:

   ```sh
   sudo ./linux/install-host-integration.sh
   sudo modprobe v4l2loopback
   ```

   You can skip this step when `v4l2loopback` and your chosen V4L2 device are
   already configured. The script is a one-time system setup, not a step to
   repeat every time the AppImage starts.

4. Open the APK on Android and start the AppImage on Linux. Choose **USB** or
   **Wi-Fi** in the Linux GUI, then follow the matching section below.

The AppImage includes the bridge application. The kernel module and udev rules
still come from the distribution and the integration script.

### CachyOS / Arch

```sh
sudo pacman -S --needed base-devel cmake pkgconf libusb ffmpeg qt6-base \
    v4l2loopback-dkms v4l2loopback-utils
```

### Ubuntu / Debian

```sh
sudo apt update
sudo apt install build-essential cmake pkg-config libusb-1.0-0-dev \
    ffmpeg libavcodec-dev libavutil-dev libswscale-dev qt6-base-dev \
    v4l2loopback-dkms v4l2loopback-utils
```

On Ubuntu, enable the repository component that provides `qt6-base-dev` and
`v4l2loopback-dkms` if your release does not include them by default. Debian
package names are the same for the packages listed above.

### Fedora

```sh
sudo dnf group install "Development Tools"
sudo dnf install cmake pkgconf-pkg-config libusb1-devel ffmpeg-free \
    ffmpeg-free-devel qt6-qtbase-devel v4l-utils
```

Fedora's base repositories do not provide the kernel module used by this
project. Enable the matching [RPM Fusion Free repository](https://rpmfusion.org/Configuration),
then install its `v4l2loopback` package:

```sh
sudo dnf install v4l2loopback
```

Other distributions need the equivalent of CMake 3.20+, a C++20 compiler,
pkg-config, libusb-1.0, FFmpeg `libavcodec`/`libavutil`/`libswscale`, Qt 6
Widgets 6.5+, and the `v4l2loopback` kernel module.

### Build from source

```sh
cmake -S host -B build/host -DCMAKE_BUILD_TYPE=Release -DAMB_BUILD_GUI=ON
cmake --build build/host --parallel
ctest --test-dir build/host --output-on-failure
./linux/build-appimage.sh
```

Run the generated AppImage with `./dist/Mobile_Webcam-<version>-x86_64.AppImage`.

## USB

1. Open Mobile Webcam on the phone and connect the USB cable.
2. In the Linux GUI, select **USB** and press **Switch to AOA** if the phone
   is still shown as a Sony device.
3. Press **Start bridge** on Linux, then **Start camera** on the phone.

The normal streaming target is accessory-only `18d1:2d00`; USB debugging is not
needed for the running camera connection.

For the equivalent terminal flow:

```fish
./build/host/amb-aoa-probe --list
sudo ./build/host/amb-aoa-probe --device 0fce:XXXX --switch
./build/host/amb-v4l2-sink --device /dev/video10 --hw-decode auto
```

## Wi-Fi

1. Press **Connect Wi-Fi** in the Android app.
2. Enter the phone's displayed IPv4 address in the Linux GUI.
3. Press **Start bridge**, then **Start camera** on the phone.

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
                                      -> Linux decode -> V4L2 /dev/videoX
```

USB and Wi-Fi share the same framed H.264 session, decoder, recovery, transform,
and V4L2 path. The AMB1 wire format and `amb-*` helper names are internal
identifiers retained for compatibility.

## Verified state

On 2026-09-17, the current Release APK was verified on a Sony Xperia XQ-GE44:

- USB AOA `18d1:2d00`: G0 PASS, G0.5 20/20 PASS, and G3 60 frames at
  1280x720@30 with VAAPI and zero discontinuities.
- Host CTest: 4/4 PASS (`wire`, `yuyv`, `image-transform`, `tcp`).
- Android `assembleRelease lintRelease` and APK signature verification: PASS.
- AppImage build and AppStream validation: PASS.

The current no-PIN Wi-Fi path has host protocol coverage. A new long-duration
Wi-Fi throughput/reconnect run and OBS/Chromium/Firefox/Discord checks remain
separate device gates.

More detail is in [`docs/architecture.md`](docs/architecture.md),
[`docs/gates.md`](docs/gates.md), [`docs/RELEASE.md`](docs/RELEASE.md), and
[`docs/evidence/`](docs/evidence/).

## License

Repository source, scripts, and documentation are MIT licensed. FFmpeg, Qt,
libusb, and v4l2loopback retain their upstream licenses; see
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) and
[`linux/APPIMAGE_LICENSES.md`](linux/APPIMAGE_LICENSES.md).
