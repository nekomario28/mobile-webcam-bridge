# Third-party notices and license boundary

Repository-owned source, scripts, and documentation are released under the
MIT license in `LICENSE`. The Windows host includes Unity Capture shared-memory
source under its upstream MIT license; its exact source identity and changes
are recorded in `host/third_party/unity_capture/`.

The host uses these dependencies:

| Component | Use | Upstream license |
| --- | --- | --- |
| libusb | Dynamically linked by all AOA host programs | LGPL-2.1-or-later |
| FFmpeg `libavcodec`, `libavutil`, `libswscale` | Dynamically linked by the video receiver | LGPL-2.1-or-later by default; GPL applies when the selected FFmpeg build enables GPL components |
| v4l2loopback | Separate kernel module providing `/dev/video10` | GPL-2.0-or-later |
| Qt 6 Base / Widgets | Dynamically linked by the desktop GUI | LGPL-3.0-only, GPL-3.0-only, or commercial terms; the open-source builds use LGPL-3.0 |
| Unity Capture | Windows DirectShow filter and shared-memory protocol | MIT; no Unity runtime/plugin is used |

Windows uses the same dynamically linked Qt, libusb and FFmpeg libraries.
The Windows build script packages the pinned Unity Capture filter DLLs and the
MSYS2 runtime dependency closure. MSYS2's GPL-enabled FFmpeg makes the combined
Windows binary distribution subject to GPL terms; repository-owned source
remains MIT. See `windows/README.md` before distributing a Windows build.

Source checkouts do not vendor those libraries or the kernel module. The
AppImage bundles Qt, FFmpeg, libusb and the C++ runtime; v4l2loopback remains
separate. The GPL-enabled combined binary distributions are provided under
GPL-3.0 terms while repository-owned source files retain their MIT license.
Exact dependency versions, package copyright notices and corresponding source
archives accompany the release. See `linux/APPIMAGE_LICENSES.md` for the Linux
boundary. Windows Setup.exe uses NSIS; its source and license notices are in
the Windows dependency-source archive.

Research references and the clean-room boundary are recorded in
`docs/provenance.md`. If source is later copied or adapted, its upstream commit,
file path, license notice, and local destination must be added here before
merge. GPL/AGPL research projects remain mechanism references only.
