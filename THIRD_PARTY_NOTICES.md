# Licensing and third-party notices

Repository-owned source, scripts and documentation in this revision use
[Apache-2.0](LICENSE), except files carrying another license notice. Attribution
is in [NOTICE](NOTICE). Commercial use,
paid distribution and proprietary modifications are allowed subject to its
terms. [Historical project-owned releases](docs/licenses/HISTORICAL-LICENSE-GRANT.md)
additionally offer Apache-2.0 while retaining their original
[MIT permission](docs/licenses/MIT-before-Apache.txt). Third-party licenses are
unchanged by either grant.

## Current Rust desktop and Android

| Component | Boundary | License / pin |
| --- | --- | --- |
| **FFmpeg** (minimal shared build) | Dynamically linked H.264 decoder + swscale + avutil; **no** `--enable-gpl` / `--enable-nonfree` | **LGPL-2.1-or-later**. Linux **7.1.5**, Windows **9.0.2**. Recipe + offer: [FFMPEG_SOURCE_OFFER.md](docs/licenses/FFMPEG_SOURCE_OFFER.md); builder `host-rs/tools/build_ffmpeg.py` |
| **libusb** (via `rusb` 0.9.4) | Dynamically linked USB access | LGPL-2.1-or-later |
| **v4l2loopback** | Separately installed Linux kernel module, not bundled; accessed through V4L2 | GPL-2.0-or-later |
| **Unity Capture filter** | Bundled Windows virtual camera DLLs (pinned checksums in `windows/unity-capture.sha256`); adapted sender in [the vendor directory](host/third_party/unity_capture/) | MIT |
| **Rust crates** (direct runtime pins from `host-rs/Cargo.toml`) | Full license texts collected into each desktop package by `host-rs/tools/package_licenses.py` | Multiple (Apache-2.0 / MIT / BSD / OFL / …). Preserve crate and font notices |
| Kotlin standard library | Android runtime; build tools retain their own licenses | Apache-2.0 |
| System runtime libraries | Platform-specific bundled closure; retain copyright notices and any source obligations, including GCC runtime exceptions | Component-specific |

### Direct Rust dependency pins (host-rs, default features)

These are the **declared** versions in `host-rs/Cargo.toml` (not the full
transitive closure). Release packages ship per-crate license files for the
resolved tree.

| Crate | Version req | Role |
| --- | --- | --- |
| eframe / egui stack | =0.36.2 | GUI |
| wgpu | =30.0.1 | GPU backend for eframe |
| ffmpeg-sys-next | =9.0.0 | FFmpeg FFI (links the minimal build above) |
| rusb | =0.9.4 | libusb |
| rustls | =0.23.46 | TLS 1.3 Wi-Fi |
| ring | =0.17.14 | crypto (rustls) |
| sha2 | =0.10.9 | hashing |
| serde / serde_json | =1.0.229 / =1.0.151 | settings / IPC |
| fontdb | =0.24.0 | fonts |
| image | =0.25.10 | optional capture helpers |
| sys-locale | =0.3.2 | UI locale |
| libc | =0.2.190 | Unix |
| windows-sys | =0.61.2 | Windows |
| pipewire-vircam | =0.7.1 | optional PipeWire camera (`pipewire` feature) |
| io-budget | git `rev=65df550` | I/O budgeting |

Embedded fonts via `epaint_default_fonts` retain OFL/UFL/MIT notices inside the
license tree produced at packaging time.

The Rust desktop does not use Qt or embed OBS. It implements the receiver and
camera sender, reusing existing virtual camera implementations. The Unity
runtime and Unity Capture **plugin** are not used. NSIS builds the Windows
installer; it is not an application runtime dependency.

Apache-2.0 does not replace LGPL obligations. Binary distributors must provide
corresponding library source and build instructions, retain notices, and allow
replacement/debugging of the LGPL libraries. Upstream download links alone
are not the project's completed corresponding-source distribution.
See the [FFmpeg guidance](https://ffmpeg.org/legal.html) and this repository's
[written offer](docs/licenses/FFMPEG_SOURCE_OFFER.md).

The [v0.2.0 audit](docs/evidence/2026-10-06-license-audit.md) records verified
artifact identities and earlier source/notice gaps. The additional historical
permission covers project-owned code only; it does not relicense dependencies.
H.264 patent permissions are a separate question from source copyright licenses.

`host-rs/tools/check_ffmpeg_source_offer.py` fails if the offer document drifts
from the pins in `build_ffmpeg.py`.

## Classic Qt releases

Classic v0.1.2 packages use LGPL Qt and GPL-enabled system FFmpeg; their combined
binary distributions use GPL-3.0 terms. Their repository-owned source remains
MIT, with additional Apache-2.0 permission for project-owned code under the
historical grant. Consult the [classic Windows guide](windows/README.md),
[AppImage notice](linux/APPIMAGE_LICENSES.md) and the dependency-source archives
of that release. Do not substitute those sources for a different Rust build.

Research-only GPL/AGPL donors and copied MIT source are distinguished in
[provenance](docs/provenance.md). Before importing source, retain its upstream
identity, license and attribution; a separate tool's presence does not by
itself determine this application's license.
