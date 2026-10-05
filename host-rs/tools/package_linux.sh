#!/bin/sh
# Run in the Debian 13 builder after build_linux.sh and package_licenses.py.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
: "${FFMPEG_DIR:?Minimal FFmpeg prefix required}"
: "${RUST_BIN_DIR:?Directory containing both release executables required}"
: "${RUST_LICENSES:?Collected Linux runtime licenses required}"
: "${LINUXDEPLOY:?Pinned linuxdeploy AppImage required}"
: "${PACKAGE_OUTPUT:?Output directory required}"
command -v file >/dev/null
printf '%s  %s\n' '8aea8da0f7f7039d2a2cecb14657d752a222a5e1d3825caeef186c82f751cdd1' "$LINUXDEPLOY" | sha256sum -c -
name=Mobile_Webcam-0.2.0-linux-x86_64-rust-preview
stage="$PACKAGE_OUTPUT/$name.AppDir"
if [ -e "$stage" ]; then
    echo "Choose a new staging directory: $stage" >&2
    exit 1
fi
mkdir -p "$stage/usr/bin" "$stage/usr/share/mobile-webcam/linux" "$stage/usr/share/doc/mobile-webcam/licenses"
install -m0755 "$RUST_BIN_DIR/mobile-webcam" "$stage/usr/bin/"
# The root-installed USB helper relies only on the distribution's libusb.
install -m0755 "$RUST_BIN_DIR/mobile-webcam-usb" "$stage/usr/share/mobile-webcam/linux/"
install -m0755 "$root/linux/install-host-integration.sh" "$stage/usr/share/mobile-webcam/linux/"
install -m0644 "$root/host-rs/linux/70-mobile-webcam.rules" "$root/linux/mobile-webcam-modules-load.conf" "$root/linux/mobile-webcam-v4l2loopback.conf" "$stage/usr/share/mobile-webcam/linux/"
install -Dm0644 "$root/linux/mobile-webcam.desktop" "$stage/usr/share/applications/mobile-webcam.desktop"
install -Dm0644 "$root/linux/mobile-webcam.svg" "$stage/usr/share/icons/hicolor/scalable/apps/mobile-webcam.svg"
cp -a "$RUST_LICENSES" "$stage/usr/share/doc/mobile-webcam/licenses/rust"
install -m0644 "$root/LICENSE" "$root/NOTICE" "$root/THIRD_PARTY_NOTICES.md" "$root/host-rs/README.md" "$stage/usr/share/doc/mobile-webcam/"
install -m0644 "$root/linux/licenses/LGPL-2.1-or-later.txt" "$FFMPEG_DIR/build-manifest.json" "$stage/usr/share/doc/mobile-webcam/licenses/"
install -m0755 "$root/host-rs/tools/build_ffmpeg.py" "$stage/usr/share/doc/mobile-webcam/"
printf '%s\n' 'FFmpeg source: https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz' > "$stage/usr/share/doc/mobile-webcam/licenses/FFMPEG_SOURCE.txt"
# Reuse the established AppImage dependency resolver without its Qt plugin path.
APPIMAGE_EXTRACT_AND_RUN=1 LD_LIBRARY_PATH="$FFMPEG_DIR/lib" "$LINUXDEPLOY" --appdir "$stage" \
    --library "$FFMPEG_DIR/lib/libavcodec.so.61" --library "$FFMPEG_DIR/lib/libavutil.so.59" --library "$FFMPEG_DIR/lib/libswscale.so.8" \
    --library "$(pkg-config --variable=libdir libusb-1.0)/libusb-1.0.so.0" \
    --library "$(pkg-config --variable=libdir xkbcommon)/libxkbcommon.so.0" \
    --library "$(c++ -print-file-name=libstdc++.so.6)" --library "$(cc -print-file-name=libgcc_s.so.1)"
python3 "$root/host-rs/tools/verify_distribution.py" linux "$stage"
APPIMAGE_EXTRACT_AND_RUN=1 LD_LIBRARY_PATH="$FFMPEG_DIR/lib" \
    LDAI_OUTPUT="$PACKAGE_OUTPUT/$name.AppImage" LINUXDEPLOY_OUTPUT_VERSION=0.2.0 \
    "$LINUXDEPLOY" --appdir "$stage" --output appimage
(cd "$PACKAGE_OUTPUT" && sha256sum "$name.AppImage" > "$name.AppImage.sha256")
