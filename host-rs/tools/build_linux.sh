#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
: "${FFMPEG_DIR:?Set FFMPEG_DIR to the minimal FFmpeg 7.1.5 prefix}"
export LD_LIBRARY_PATH="$FFMPEG_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export MOBILE_WEBCAM_REQUIRE_MINIMAL=1
export CARGO_BUILD_JOBS=1
cargo test --manifest-path "$root/Cargo.toml" --locked
cargo build --manifest-path "$root/Cargo.toml" --release --bin mobile-webcam --locked
cargo build --manifest-path "$root/Cargo.toml" --release --no-default-features --bin mobile-webcam-usb --locked
