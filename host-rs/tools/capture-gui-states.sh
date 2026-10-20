#!/usr/bin/env bash
# Capture the desktop GUI's states from the recorded fixtures, on a headless X.
#
# The states come from `tests/fixtures/capture/*.json`, replayed through the real
# session by `Session::for_capture`, so what a capture shows is what those events
# produce. The pixel capture itself needs a display the process can draw to:
#   * a dedicated Xvfb screen with a window manager, so the window is mapped and
#     placed where the screenshot expects it (`MW_CAPTURE_POSITION`),
#   * WAYLAND_DISPLAY *cleared*: with it set, the toolkit picks Wayland and
#     nothing lands on the X display being captured (this cost two separate
#     investigations),
#   * LANGUAGE as well as LC_ALL for the locale, because the Unix locale provider
#     prioritises LANGUAGE over LC_ALL.
set -euo pipefail
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${1:-"$root/target/gui-captures"}
binary=${MW_CAPTURE_BINARY:-"$root/target/debug/mobile-webcam"}
# Build it here, with the capture feature. The binary shares its path with the
# ordinary `--features video` build, so a stale one silently ignores
# MW_CAPTURE_FIXTURE/MW_SCREENSHOT and the app just runs a live session for ever.
if [ -z "${MW_CAPTURE_NO_BUILD:-}" ]; then
  echo "building the capture binary (--features capture)"
  (cd "$root" && cargo build --locked --features capture >/dev/null)
fi
# Start from an empty directory: the manifest is built from every PNG in it, so a
# stale capture from an earlier run would be counted as a state the current
# fixtures no longer produce (measured: a deleted fixture kept passing as one of
# twelve "states").
rm -rf "$out"
mkdir -p "$out"
display=90
for lang in en ja; do
  for fixture in "$root"/tests/fixtures/capture/*.json; do
    name=$(basename "$fixture" .json)
    # A display that is already taken makes Xvfb exit and the app then never
    # produces its screenshot, which hangs the capture; pick a free one.
    display=$((display + 1))
    while [ -e "/tmp/.X11-unix/X$display" ]; do display=$((display + 1)); done
    Xvfb ":$display" -screen 0 640x1000x24 -nolisten tcp >"$out/xvfb-$lang-$name.log" 2>&1 &
    xvfb=$!
    # Wait for the server to answer rather than for a fixed delay: under load a
    # fixed sleep starts the app against a display that is not up yet, and then no
    # frame is ever presented and no screenshot is produced.
    ready=0
    for _ in $(seq 1 40); do
      if xdpyinfo -display ":$display" >/dev/null 2>&1; then ready=1; break; fi
      sleep 0.5
    done
    if [ "$ready" != 1 ]; then
      echo "display :$display never came up" >&2
      kill "$xvfb" 2>/dev/null || true
      continue
    fi
    DISPLAY=":$display" openbox >"$out/openbox-$lang-$name.log" 2>&1 &
    wm=$!
    # A window manager is what gets the window mapped where MW_CAPTURE_POSITION
    # asks for it; give it a moment, and fail loudly if it died.
    sleep 1.5
    if ! kill -0 "$wm" 2>/dev/null; then
      echo "window manager died on :$display (see $out/openbox-$lang-$name.log)" >&2
      kill "$xvfb" 2>/dev/null || true
      continue
    fi
    if [ "$lang" = ja ]; then locale_env=(LANGUAGE=ja_JP:ja LC_ALL=ja_JP.UTF-8); else locale_env=(LANGUAGE=en_US:en LC_ALL=C); fi
    env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET DISPLAY=":$display" "${locale_env[@]}" \
      MW_CAPTURE_FIXTURE="$fixture" MW_CAPTURE_DETAILS=1 MW_CAPTURE_POSITION=0,0 \
      MW_PREVIEW_YUYV="$root/tests/fixtures/capture/preview.yuyv" MW_PREVIEW_SIZE=320x180 \
      MW_SCREENSHOT="$out/$lang-$name.png" timeout 60 "$binary" >"$out/gui-$lang-$name.log" 2>&1 || true
    if [ ! -s "$out/$lang-$name.png" ]; then
      echo "capture missing after 60s: $lang-$name (see $out/gui-$lang-$name.log)" >&2
    fi
    kill "$wm" "$xvfb" 2>/dev/null || true
    wait "$wm" "$xvfb" 2>/dev/null || true
  done
done
python3 - "$out" <<'PY'
import hashlib, itertools, json, pathlib, struct, sys
out = pathlib.Path(sys.argv[1])
frames = sorted(out.glob("*.png"))
if not frames:
    raise SystemExit("no captures were produced")
manifest = {}
for frame in frames:
    data = frame.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and data[12:16] == b"IHDR", frame
    width, height = struct.unpack(">II", data[16:24])
    manifest[frame.stem] = {
        "sha256": hashlib.sha256(data).hexdigest(),
        "width": width,
        "height": height,
    }
(out / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True))
print(f"captured {len(frames)} states into {out}")
for name in sorted(manifest):
    print(f"  {name}: {manifest[name]['width']}x{manifest[name]['height']}")

# A capture set is only evidence if the states are actually distinguishable: two
# fixtures that render the same frame would let a state mapping regress silently.
# Whole-frame brightness is far too coarse here (a one-line status change moves it
# by 0.2), so this counts pixels that changed by more than 12 levels.
try:
    from PIL import Image
    import numpy as np
except ImportError:
    print("PIL/numpy absent: skipping the distinguishability check")
    raise SystemExit(0)
languages = sorted({name.split("-")[0] for name in manifest})
fixtures = sorted({name.split("-", 1)[1] for name in manifest})
minimum = 200
same_state = 0
for language in languages:
    for first, second in itertools.combinations(fixtures, 2):
        left = np.asarray(Image.open(out / f"{language}-{first}.png").convert("L")).astype(int)
        right = np.asarray(Image.open(out / f"{language}-{second}.png").convert("L")).astype(int)
        changed = int((np.abs(left - right) > 12).sum())
        if changed < minimum:
            same_state += 1
            print(f"  SAME STATE: {language}-{first} vs {language}-{second}: {changed} changed pixels")
        elif changed < 1000:
            print(f"  near: {language}-{first} vs {language}-{second}: {changed} changed pixels")
print(f"states checked: {len(languages) * len(fixtures)} captures, {same_state} indistinguishable pair(s)")
assert same_state == 0, "two fixtures render the same frame"
PY
