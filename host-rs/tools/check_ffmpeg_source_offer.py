#!/usr/bin/env python3
"""Fail if docs/licenses/FFMPEG_SOURCE_OFFER.md drifts from build_ffmpeg.py pins."""
from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILD = (ROOT / "host-rs/tools/build_ffmpeg.py").read_text()
OFFER = (ROOT / "docs/licenses/FFMPEG_SOURCE_OFFER.md").read_text()

pins = {plat: (ver, sha) for plat, ver, sha in re.findall(r'"(\w+)":\("([^"]+)","([^"]+)"\)', BUILD)}
nv_rev = re.search(r'NV_REV="([^"]+)"', BUILD).group(1)
nv_sha = re.search(r'NV_SHA="([^"]+)"', BUILD).group(1)

errors = []
for platform, (version, sha) in pins.items():
    if version not in OFFER:
        errors.append(f"offer missing FFmpeg version {version} ({platform})")
    if sha not in OFFER:
        errors.append(f"offer missing sha {sha} ({platform})")
if nv_rev not in OFFER:
    errors.append(f"offer missing nv-codec rev {nv_rev}")
if nv_sha not in OFFER:
    errors.append(f"offer missing nv-codec sha {nv_sha}")
for flag in (
    "--disable-avformat",
    "--enable-decoder=h264",
    "--enable-shared",
    "build_ffmpeg.py",
):
    if flag not in OFFER:
        errors.append(f"offer missing required token {flag!r}")

if errors:
    print("FFMPEG_SOURCE_OFFER.md out of sync:", file=sys.stderr)
    for e in errors:
        print(f"  - {e}", file=sys.stderr)
    sys.exit(1)
print("FFMPEG_SOURCE_OFFER.md matches build_ffmpeg.py pins")
