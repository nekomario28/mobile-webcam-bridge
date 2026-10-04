#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
: "${FFMPEG_DIR:?Set FFMPEG_DIR to the minimal FFmpeg 9.0.2 prefix}"
export CARGO_BUILD_JOBS=1
cargo test --manifest-path "$root/Cargo.toml" --target x86_64-pc-windows-gnu --no-run --locked
cargo build --manifest-path "$root/Cargo.toml" --target x86_64-pc-windows-gnu --release --bin mobile-webcam --locked
