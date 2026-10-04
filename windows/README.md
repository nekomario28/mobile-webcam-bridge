# Mobile Webcam on Windows

The Windows x86_64 host uses the same Android APK and USB/Wi-Fi protocol as
Linux. It publishes video through the MIT-licensed
[Unity Capture](https://github.com/schellingb/UnityCapture) DirectShow filter;
Unity itself is not required. Windows support is experimental until the
real-device checks below have passed. The existing v0.1.1 release has no Windows ZIP.

## Use a Windows build

1. Extract the ZIP into a permanent folder. Right-click
   `virtual-camera/install-camera.bat` and choose **Run as administrator** once.
2. Open `mobile-webcam.exe`. For the simplest connection, choose **Wi-Fi / LAN**,
   enter the IP shown by **Connect Wi-Fi** in the Android app, and start the bridge.
3. Start the camera on Android. In your camera application, select
   **Unity Video Capture**. The bridge initially waits until a camera application opens it.

Use the existing trusted-LAN requirements in the project's Wi-Fi instructions.
To move or delete the installed folder, first run
`virtual-camera/uninstall-camera.bat` as administrator. Registration refers to
the DLLs in that folder. Another Unity Capture installation uses the same
camera slot; do not run its producer alongside Mobile Webcam.

## USB

USB debugging remains unnecessary for the Android stream. Windows must provide
a libusb-compatible USB driver; see the
[libusb Windows documentation](https://github.com/libusb/libusb/wiki/Windows).
Use **Switch to AOA** when the original phone interface is accessible to libusb.
After switching, accessory-only mode has a different identity (`18d1:2d00`) and
may need its own WinUSB binding. If switching is blocked by the original driver,
use Wi-Fi until the exact phone/interface driver setup is verified. Do not
replace an unrelated USB device's driver.

## Build

Install [MSYS2](https://www.msys2.org/) and open its **UCRT64** shell. Update MSYS2
as instructed by that project, then install the build dependencies:

```sh
pacman -S --needed mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-cmake \
  mingw-w64-ucrt-x86_64-ninja mingw-w64-ucrt-x86_64-pkgconf \
  mingw-w64-ucrt-x86_64-libusb mingw-w64-ucrt-x86_64-ffmpeg \
  mingw-w64-ucrt-x86_64-qt6-base curl
bash windows/build-windows.sh
```

MINGW64 is also supported with the corresponding `mingw-w64-x86_64-*` packages.
The script builds, runs CTest, verifies the pinned camera DLLs and creates a ZIP
in `dist/`. All tools and libraries must come from the same MSYS2 environment.
The test suite checks framing, transforms, TCP deadlines/partial reads and Windows
shared-memory exchange. It does not prove that another application can use the camera.

## Before a Windows release

On actual Windows hardware, confirm Android Wi-Fi and USB with debugging off,
camera enumeration and moving video in the intended consumer applications,
live rotation/flip, stop/start and reconnect. Check a clean machine without
MSYS2 on PATH to ensure that the ZIP includes its runtime dependencies.

The MSYS2 FFmpeg package enables GPL components. A binary distributor must
follow that package's GPL terms and provide the corresponding source, including
its build recipe, along with the Qt/libusb license requirements. Package license
texts are included in the ZIP. Exact package recipes and source locations are
maintained by [MSYS2 MINGW-packages](https://github.com/msys2/MINGW-packages);
record the dependency versions from the build environment before publishing.
