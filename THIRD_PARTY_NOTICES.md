# Third-party notices and license boundary

Repository-owned source, scripts, and documentation are released under the
MIT license in `LICENSE`. No third-party source code is copied into this
repository.

The Linux programs use these separately installed dependencies:

| Component | Use | Upstream license |
| --- | --- | --- |
| libusb | Dynamically linked by all AOA host programs | LGPL-2.1-or-later |
| FFmpeg `libavcodec`, `libavutil`, `libswscale` | Dynamically linked by `amb-v4l2-sink` | LGPL-2.1-or-later by default; GPL applies when the selected FFmpeg build enables GPL components |
| v4l2loopback | Separate kernel module providing `/dev/video10` | GPL-2.0-or-later |
| Qt 6 Base / Widgets | Dynamically linked by the optional Linux GUI | LGPL-3.0-only, GPL-3.0-only, or commercial terms; the open-source AppImage build uses LGPL-3.0 |

Source checkouts do not vendor those libraries or the kernel module. The
optional AppImage bundles Qt and FFmpeg userspace libraries while using the
host's libusb runtime; its
`APPIMAGE_LICENSES.md` records that binary distribution boundary. A distributor
must comply with the exact licenses and configuration of the dependency builds
it ships. In particular, the CachyOS/Arch FFmpeg package used here reports
`GPL-3.0-only`, so the generated AppImage is distributed under GPL-3.0-only
terms while repository-owned source files retain their MIT license.

Research references and the clean-room boundary are recorded in
`docs/provenance.md`. If source is later copied or adapted, its upstream commit,
file path, license notice, and local destination must be added here before
merge. GPL/AGPL research projects remain mechanism references only.
