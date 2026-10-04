# Mobile Webcam AppImage license notice

The Mobile Webcam source code, scripts, and documentation remain under
the MIT license in `LICENSE`.

The AppImage dynamically links userspace libraries from the build system.
linuxdeploy bundles Qt, FFmpeg and libusb libraries.
The release records exact library versions in its dependency inventory and
provides corresponding source archives alongside the binaries. The bundled
components include:

- Qt 6 Base / Widgets: LGPL-3.0-only, GPL-3.0-only, or commercial terms. This
  open-source build uses the LGPL-3.0 option and includes the package's license
  texts under `usr/share/doc/mobile-webcam/licenses` and package copyright
  notices under `usr/share/doc/mobile-webcam/dependency-licenses`.
- libusb: LGPL-2.1-or-later (bundled userspace library).
- FFmpeg libraries: the Debian 13 build enables GPL components and reports
  GPL version 2 or later. This combined AppImage distribution uses GPL-3.0
  terms while repository-owned source files retain their MIT license.

v4l2loopback is a separately installed GPL-2.0-or-later kernel module and is
not included in the AppImage.

Builds made on another system must record and comply with the exact licenses
of the libraries that linuxdeploy bundles from that system.
