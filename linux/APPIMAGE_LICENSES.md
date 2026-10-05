# AppImage licensing

The current Rust AppImage uses dynamically linked FFmpeg and libusb; it does
not include Qt or v4l2loopback. Each bundled dependency retains its own license.
See [third-party notices](../THIRD_PARTY_NOTICES.md) for the current licensing
boundary and distribution requirements.

The classic Qt v0.1.2 AppImage bundles LGPL Qt and a GPL-enabled Debian FFmpeg
build. Its combined binary distribution uses GPL-3.0 terms; the project-owned
source was published under MIT and additionally offers Apache-2.0 under the
[historical grant](../docs/licenses/HISTORICAL-LICENSE-GRANT.md). Do not apply
that FFmpeg license to the minimal LGPL build used by the Rust release.
