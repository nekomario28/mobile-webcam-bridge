# Mobile Webcam AppImage license notice

The Mobile Webcam source code, scripts, and documentation remain under
the MIT license in `LICENSE`.

The AppImage dynamically links userspace libraries from the build system.
linuxdeploy bundles Qt and FFmpeg libraries, while libusb remains a host
runtime dependency.
The current CachyOS/Arch build uses:

- Qt 6 Base / Widgets: LGPL-3.0-only, GPL-3.0-only, or commercial terms. This
  open-source build uses the LGPL-3.0 option and includes the package's license
  texts under `usr/share/doc/mobile-webcam/qt6-base-licenses`.
- libusb: LGPL-2.1-or-later (host runtime dependency; not bundled).
- FFmpeg libraries: the packaged build was configured with GPL and version 3
  components and reports GPL-3.0-only. The AppImage binary distribution is
  therefore provided under GPL-3.0-only terms while the repository-owned
  source files retain their MIT license.

v4l2loopback is a separately installed GPL-2.0-or-later kernel module and is
not included in the AppImage.

Builds made on another system must record and comply with the exact licenses
of the libraries that linuxdeploy bundles from that system.
