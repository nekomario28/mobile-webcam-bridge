# Mobile Webcam on Windows

The Windows x86_64 host uses the same Android APK and USB/Wi-Fi protocol as
Linux. It publishes video through the MIT-licensed
[Unity Capture](https://github.com/schellingb/UnityCapture) DirectShow filter;
Unity itself is not required. Windows support is experimental until the
real-device checks below have passed. The v0.1.2 release includes the experimental installer.

## Use a Windows build

1. Run `Mobile_Webcam-<version>-windows-x86_64-experimental-Setup.exe` and accept
   the Windows administrator prompt. It installs the app and virtual camera together;
   no BAT files, terminal commands, MSYS2 or Unity installation are needed to use it.
2. Open **Mobile Webcam** from the Start menu. **Wi-Fi / LAN** is selected initially;
   enter the IP shown by **Wi-Fi** in the Android app and press **Start**.
   The IP and connection mode are restored on subsequent launches.
3. Start the camera on Android. In your camera application, select
   **Unity Video Capture**. The bridge initially waits until a camera application opens it.

Use the existing trusted-LAN requirements in the project's Wi-Fi instructions.
Remove it through Windows **Installed apps** or **Mobile Webcam → Uninstall**
in the Start menu. Close Mobile Webcam and camera applications before updating
or removing it. The installer refuses to overwrite another Unity Capture
installation; remove that installation with its own tools first. Registration
refers to the installed DLLs, so use the uninstaller before moving its folder.
Uninstall deletes the packaged files and preserves files you added yourself.
Another Unity Capture producer uses the same camera slot; do not run it alongside
Mobile Webcam. The ZIP is a build artifact; use Setup.exe for installation.

## USB

USB debugging remains unnecessary for the Android stream. Windows must provide
a libusb-compatible USB driver; see the
[libusb Windows documentation](https://github.com/libusb/libusb/wiki/Windows).
Press **Start** in USB mode when the original phone interface is accessible to
libusb; the app switches to AOA and starts the receiver.
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
  mingw-w64-ucrt-x86_64-qt6-base mingw-w64-ucrt-x86_64-nsis curl
bash windows/build-windows.sh
```

MINGW64 is also supported with the corresponding `mingw-w64-x86_64-*` packages.
The script builds, runs CTest, verifies the pinned camera DLLs and creates a ZIP
and a self-contained NSIS Setup.exe in `dist/`. All tools and libraries must come
from the same MSYS2 environment. To repeat packaging, remove only its previous
`dist/Mobile_Webcam-<version>-windows-x86_64-experimental` staging folder first.
The test suite checks framing, transforms, TCP deadlines/partial reads and Windows
shared-memory exchange. It does not prove that another application can use the camera.

## Windows hardware checks

On actual Windows hardware, confirm Android Wi-Fi and USB with debugging off,
camera enumeration and moving video in the intended consumer applications,
live rotation/flip, stop/start and reconnect. Check a clean machine without
MSYS2 on PATH to ensure that Setup.exe includes its runtime dependencies.

The MSYS2 FFmpeg package enables GPL components. A binary distributor must
follow that package's GPL terms and provide the corresponding source, including
its build recipe, along with the Qt/libusb license requirements. Package license
texts are included in Setup.exe and the ZIP. Exact package recipes and source locations are
maintained by [MSYS2 MINGW-packages](https://github.com/msys2/MINGW-packages);
record the dependency versions from the build environment before publishing.
