#!/usr/bin/env bash
set -euo pipefail

if [[ "${MSYSTEM:-}" != MINGW64 && "${MSYSTEM:-}" != UCRT64 ]]; then
    echo "Run this script in an MSYS2 MINGW64 or UCRT64 shell." >&2
    exit 1
fi
repo=$(cd "$(dirname "$0")/.." && pwd)
cd "$repo"
cmake -S host -B build/windows -G Ninja -DCMAKE_BUILD_TYPE=Release -DAMB_BUILD_GUI=ON
cmake --build build/windows --parallel
ctest --test-dir build/windows --output-on-failure

# Pin and verify the external DirectShow filter; no Unity runtime is required.
mkdir -p build/unity-capture
revision=3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2
for bits in 32 64; do
    curl --fail --location --retry 2 \
        "https://raw.githubusercontent.com/schellingb/UnityCapture/$revision/Install/UnityCaptureFilter$bits.dll" \
        -o "build/unity-capture/UnityCaptureFilter$bits.dll"
done
(cd build/unity-capture && sha256sum --check "$repo/windows/unity-capture.sha256")
cmake -DAMB_BUILD_DIR="$repo/build/windows" -DAMB_MINGW_ROOT="$MINGW_PREFIX" \
      -DAMB_FILTER_DIR="$repo/build/unity-capture" -DAMB_OUTPUT_DIR="$repo/dist" \
      -P windows/package.cmake
