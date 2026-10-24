#!/bin/sh
# Build the current (Rust) Linux AppImage for GitHub Releases.
# Deliberately does NOT call linux/build-appimage.sh (classic Qt + GPL FFmpeg).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(cat "$root/VERSION")
dist="$root/dist"
ffmpeg_prefix="${FFMPEG_DIR:-$root/host-rs/target/native-linux-release}"
cache="${FFMPEG_SOURCE_CACHE:-$root/host-rs/target/ffmpeg-source}"
tool_dir="$root/build/appimage-tools"
linuxdeploy="$tool_dir/linuxdeploy-x86_64.AppImage"
linuxdeploy_url="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
linuxdeploy_sha="8aea8da0f7f7039d2a2cecb14657d752a222a5e1d3825caeef186c82f751cdd1"

mkdir -p "$tool_dir" "$dist" "$cache"
if [ ! -f "$linuxdeploy" ] || ! printf '%s  %s\n' "$linuxdeploy_sha" "$linuxdeploy" | sha256sum -c - >/dev/null 2>&1; then
    curl --fail --location --output "$linuxdeploy.download" "$linuxdeploy_url"
    printf '%s  %s\n' "$linuxdeploy_sha" "$linuxdeploy.download" | sha256sum -c -
    mv "$linuxdeploy.download" "$linuxdeploy"
    chmod +x "$linuxdeploy"
fi

if [ ! -d "$ffmpeg_prefix/lib" ]; then
    rm -rf "$ffmpeg_prefix"
    python3 "$root/host-rs/tools/build_ffmpeg.py" linux --prefix "$ffmpeg_prefix" --cache "$cache"
fi
export FFMPEG_DIR="$ffmpeg_prefix"
export LD_LIBRARY_PATH="$FFMPEG_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export MOBILE_WEBCAM_REQUIRE_MINIMAL=1
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-1}"

cargo test --manifest-path "$root/host-rs/Cargo.toml" --locked
cargo build --manifest-path "$root/host-rs/Cargo.toml" --release --bin mobile-webcam --locked
cargo build --manifest-path "$root/host-rs/Cargo.toml" --release --no-default-features --bin mobile-webcam-usb --locked

licenses="$root/host-rs/target/release-licenses-linux"
rm -rf "$licenses"
if ! python3 "$root/host-rs/tools/package_licenses.py" x86_64-unknown-linux-gnu "$licenses"; then
    echo "package_licenses.py failed; installing minimal license tree" >&2
    mkdir -p "$licenses"
    install -m0644 "$root/LICENSE" "$root/NOTICE" "$root/THIRD_PARTY_NOTICES.md" "$licenses/"
fi

export RUST_BIN_DIR="$root/host-rs/target/release"
export RUST_LICENSES="$licenses"
export LINUXDEPLOY="$linuxdeploy"
export PACKAGE_OUTPUT="$dist"
# Fresh stage each run
rm -rf "$dist/Mobile_Webcam-${version}-x86_64.AppDir"
"$root/host-rs/tools/package_linux.sh"

# Guard: refuse any Qt-named classic artifact sneaking in
if [ -f "$dist/Mobile_Webcam-${version}-x86_64.AppImage" ]; then
    if command -v strings >/dev/null; then
        if strings "$dist/Mobile_Webcam-${version}-x86_64.AppImage" | grep -E -q 'libQt6Core|Qt_6'; then
            echo "Refusing release: AppImage appears to contain Qt (classic path)" >&2
            exit 3
        fi
    fi
fi
echo "Rust AppImage ready: $dist/Mobile_Webcam-${version}-x86_64.AppImage"
