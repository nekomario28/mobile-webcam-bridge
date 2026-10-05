# Licensing and third-party notices

Repository-owned source, scripts and documentation in this revision use
[Apache-2.0](LICENSE), except files carrying another license notice. Attribution
is in [NOTICE](NOTICE). Commercial use,
paid distribution and proprietary modifications are allowed subject to its
terms. Releases through v0.2.0 retain their original
[MIT license](docs/licenses/MIT-before-Apache.txt). Third-party licenses are
unchanged by this choice.

## Current Rust desktop and Android

| Component | Boundary | License |
| --- | --- | --- |
| FFmpeg | Dynamically linked H.264 decoder/converter; minimal build without `--enable-gpl` or `--enable-nonfree` | LGPL-2.1-or-later |
| libusb | Dynamically linked USB access library | LGPL-2.1-or-later |
| v4l2loopback | Separately installed Linux kernel module, not bundled; accessed through V4L2 | GPL-2.0-or-later |
| Unity Capture filter | Bundled Windows virtual camera; adapted sender attribution is retained in [the vendor directory](host/third_party/unity_capture/) | MIT |
| Rust crates and embedded fonts | Target-specific runtime inventory and license texts in each desktop package | Multiple licenses; preserve font and crate notices |
| Kotlin standard library | Android runtime; build tools retain their own licenses | Apache-2.0 |
| System runtime libraries | Platform-specific bundled closure; retain copyright notices and any source obligations, including GCC runtime exceptions | Component-specific |

The Rust desktop does not use Qt or embed OBS. It implements the receiver and
camera sender, reusing existing virtual camera implementations. The Unity
runtime and Unity Capture plugin are not used. NSIS builds the Windows
installer; it is not an application runtime dependency.

Apache-2.0 does not replace LGPL obligations. Binary distributors must provide
corresponding library source and build instructions, retain notices, and allow
replacement/debugging of the LGPL libraries. Upstream download links alone
are not the project's completed corresponding-source distribution.
See the [FFmpeg guidance](https://ffmpeg.org/legal.html).

The [v0.2.0 audit](docs/evidence/2026-10-06-license-audit.md) records verified
artifact identities and remaining source/notice gaps. This source license
change does not retroactively repair or relicense those binaries. H.264 patent
permissions are a separate question from source copyright licenses.

## Classic Qt releases

Classic v0.1.2 packages use LGPL Qt and GPL-enabled system FFmpeg; their combined
binary distributions use GPL-3.0 terms. Their repository-owned source remains
MIT. Consult the [classic Windows guide](windows/README.md),
[AppImage notice](linux/APPIMAGE_LICENSES.md) and the dependency-source archives
of that release. Do not substitute those sources for a different Rust build.

Research-only GPL/AGPL donors and copied MIT source are distinguished in
[provenance](docs/provenance.md). Before importing source, retain its upstream
identity, license and attribution; a separate tool's presence does not by
itself determine this application's license.
