# License audit — 2026-10-06

## Decision and scope

Use Apache-2.0 for repository-owned source after this change: paid use,
enterprise adoption and proprietary modifications are allowed, with an
explicit contributor patent grant. MIT was also commercially usable; GPL
also allows charging and business use, but adds copyleft redistribution
conditions. This choice does not grant unrelated codec patents.

The audit inspected stable [v0.2.0](https://github.com/nekomario28/mobile-webcam-bridge/releases/tag/v0.2.0),
source `e0c0918f1bbf7ad4137901ea0053b75766e3ff51`. Its project-owned source
and binaries retain their original MIT permission. A subsequent
[historical grant](../licenses/HISTORICAL-LICENSE-GRANT.md) adds Apache-2.0 as
an alternative for project-owned material only; dependency terms and the
distribution findings below are unaffected. All Git author identities in the
fetched repository belong to the same owner account; this is not an independent copyright-title
certification. Vendor source, Gradle wrapper and dependency licenses remain
unchanged. The former MIT grant is preserved in `docs/licenses/`.

## Distribution evidence

Public files inspected, SHA-256:

| Artifact | SHA-256 |
| --- | --- |
| Linux AppImage | `0b24d662f37947d5544f48656572cbaf845995668bdf8b2fb0d2729d84e23122` |
| Windows ZIP | `631cf36180f0071c64a15ff251dd94d5705e27b2cc4c50cb345657f4566d8af4` |
| Android APK | `1ea9e383f81dc55ee33ed5ad8547fc93b13a6ed5a693637889d80e6e29e51db0` |

The [inventory](2026-10-06-license-inventory.json) records license expressions
from the shipped Rust runtime inventories and the FFmpeg manifests. Private
build prefixes are redacted; the configure options are otherwise retained.

- Linux FFmpeg 7.1.5: the extracted avutil, avcodec and swscale libraries each
  return `LGPL version 2.1 or later` from their runtime license API. All three
  report the same minimal configuration, with no GPL/nonfree enable flags.
- Windows FFmpeg 9.0.2: the bundled build manifest and DLL configuration
  strings show the minimal build without GPL/nonfree enable flags. The
  Windows runtime license API was NOT RUN; do not treat Linux calls as
  Windows runtime evidence. The ZIP's imported DLL closure contains no Qt.
- The shipped Rust inventories contain 229 Linux and 148 Windows records.
  No mandatory GPL dependency was identified in these declared expressions;
  `self_cell` offers Apache-2.0 as an alternative to GPL-2.0-only. Embedded
  fonts additionally require OFL/UFL attribution. The inventories are not
  an independent audit of every upstream source file or build tool.
- Linux reuses separately installed v4l2loopback through the kernel V4L2 API.
  Windows bundles the MIT Unity Capture filter and implements its sender.
  OBS and the Unity runtime/plugin are not embedded. See the actual output
  implementations in `host-rs/src/output/` and the retained vendor notices.

## Findings and repairs

The old top-level licensing notice described GPL-enabled Qt packages as if
it applied to the Rust release. The current notice now separates these
builds. The Rust Linux/Windows packagers now copy LICENSE, NOTICE and
THIRD_PARTY_NOTICES together. Current source adds Apache/Kotlin attribution
under Android's default `src/main/assets/licenses/` path, preserving Kotlin
2.0.21 upstream notices; their provenance is in
[the receipt](2026-10-06-android-license-sources.json).

The published v0.2.0 binaries have NOT been rebuilt or replaced by these
source changes. The following distribution issues remain open:

1. v0.2.0 has no accompanying dependency-source archives, only the application
   source archive and upstream source links/build instructions. Supply exact
   corresponding source and recipes for FFmpeg and each bundled copyleft
   runtime library. Check the complete library closure, including platform
   libraries and runtime exceptions; older Qt archives cannot be assumed to
   match this build. This source coverage is NOT VERIFIED.
2. The published Android APK contains no named license/notice assets. The
   next APK must contain the newly added assets and its actual resolved
   runtime dependency notices. APK packaging of the new assets is NOT RUN.
3. Check notices for the Linux system-library closure, not only the Cargo
   inventory. That complete notice coverage is NOT VERIFIED.

**Conclusion:** Apache-2.0 is suitable for the repository-owned source and
no need to relicense the Rust application as GPLv3 was identified. This is
not a clean bill of redistribution compliance for all v0.2.0 binaries.
Next safe action: complete the exact dependency-source/notice inventory,
publish matching source archives and verify freshly packaged notices before
advertising redistribution compliance or publishing rebuilt binaries.

## Validation of this source change

PASS: Cargo metadata in locked/offline mode, XML/JSON parsing, changed local
Markdown links, shell syntax and whitespace checks; byte preservation of the
old MIT grant and Unity Capture vendor files. The actual edited Linux install
and Windows CMake copy commands produced byte-identical LICENSE, NOTICE and
THIRD_PARTY_NOTICES files in temporary staging directories. Complete desktop
packages and APKs were NOT rebuilt; these checks do not prove new binary
notice coverage or close the corresponding-source findings above.

## Primary authorities

- [Apache-2.0 copyright, patent and redistribution terms](https://www.apache.org/licenses/LICENSE-2.0)
- [FFmpeg LGPL build and distribution guidance; codec patents](https://ffmpeg.org/legal.html)
- [Linux userspace syscall licensing boundary](https://docs.kernel.org/process/license-rules.html)
- [v4l2loopback license header](https://raw.githubusercontent.com/v4l2loopback/v4l2loopback/main/v4l2loopback.c)
- [Unity Capture filter/plugin license split](https://github.com/schellingb/UnityCapture#license)
- [GNU GPLv3 terms, including paid distribution](https://www.gnu.org/licenses/gpl-3.0.html)
