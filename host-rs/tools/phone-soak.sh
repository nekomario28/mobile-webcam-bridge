#!/bin/bash
# A ten-minute stream on the phone, with the numbers that decide things.
#
# Runs the host headless (no window needed) against the real phone and samples, once a
# minute: battery level and current, the CPU and skin thermal zones, the screen state,
# and the host's frame counter. Prints a table plus the session's JSON events.
#
# Usage:
#   host-rs/tools/phone-soak.sh [seconds] [serial]
#
# The instrument carries a control. Two of them, in fact, because a measurement that
# cannot show a change is not a measurement:
#   * a 30 s idle baseline is taken before the stream, so every delta has something to
#     be a delta *from*;
#   * the script refuses to report if the session never streamed (no stats event), and
#     prints the idle and streaming columns side by side so a flat instrument is
#     visible as flat rather than reported as "no thermal change".
#
# What it does not do: press Start camera. Capture consent is a human act; the phone
# must already be showing 配信中 when this starts, or the script will say so and stop.

set -uo pipefail

SECONDS_TO_RUN=${1:-600}
SERIAL=${2:-}
# The phone's address is DHCP and *does* change (measured: 192.168.1.14 → .15 after a
# reboot), while the PC stores the one it was paired with. Without this the run waits
# out its whole timeout talking to an address nobody is listening on.
PHONE_HOST=${PHONE_HOST:-}
# SCREEN_OFF=1 presses power once capture is confirmed, which is the only honest way to
# ask "does the stream survive the screen going off" — the app holds FLAG_KEEP_SCREEN_ON
# while its window is visible, and that flag does not stop an explicit power press.
SCREEN_OFF=${SCREEN_OFF:-0}
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ADB="adb"
[ -n "$SERIAL" ] && ADB="adb -s $SERIAL"

HOST_BIN="$ROOT/host-rs/target/release/mobile-webcam"
[ -x "$HOST_BIN" ] || HOST_BIN="$ROOT/host-rs/target/debug/mobile-webcam"

command -v adb >/dev/null || { echo "adb is required" >&2; exit 2; }
# A stale binary that predates --headless prints its usage and exits, which the first
# version of this script read as "the host is running and the phone is not capturing".
# Check the flag exists before blaming the phone.
[ -x "$HOST_BIN" ] || { echo "no host binary; build it first (cargo build --release)" >&2; exit 2; }
"$HOST_BIN" --help 2>/dev/null | grep -q -- '--headless' || {
  echo "$HOST_BIN does not support --headless; rebuild it (cargo build --release)" >&2
  exit 2
}
$ADB shell true >/dev/null 2>&1 || { echo "no device on $ADB (pass a serial)" >&2; exit 2; }

# One reading. Fields: level%, charge counter (uAh), power source, battery C, cpu C,
# skin C, screen state, and session-active yes/no.
#
# Two unit traps on this device, both measured rather than assumed:
#   * `dumpsys battery` reports temperature in TENTHS of a degree (311 = 31.1 C);
#   * `/sys/class/thermal/thermal_zone*/temp` reports MILLI-degrees (36600 = 36.6 C),
#     and unpopulated sensors read -273000. Dividing those by 1000 is the difference
#     between "36.6 C" and "9500 C", which the first version of this script printed.
sample() {
  local battery level counter temp ac source read_cpu read_skin screen active
  battery=$($ADB shell dumpsys battery 2>/dev/null)
  level=$(printf '%s\n' "$battery" | sed -n 's/^  level: \([0-9]*\)$/\1/p' | head -1)
  counter=$(printf '%s\n' "$battery" | sed -n 's/^  Charge counter: \([0-9-]*\)$/\1/p' | head -1)
  temp=$(printf '%s\n' "$battery" | sed -n 's/^  temperature: \([0-9-]*\)$/\1/p' | head -1)
  if printf '%s\n' "$battery" | grep -q '^  AC powered: true'; then
    source=AC
  elif printf '%s\n' "$battery" | grep -q '^  USB powered: true'; then
    source=USB
  elif printf '%s\n' "$battery" | grep -q '^  Wireless powered: true'; then
    source=wireless
  else
    source=battery
  fi
  read_cpu=$($ADB shell 'm=0
for z in /sys/class/thermal/thermal_zone*; do
  t=$(cat $z/type 2>/dev/null); v=$(cat $z/temp 2>/dev/null)
  # `cpu-hw-trip-*` reads 95000: that is a hardware trip THRESHOLD, not a temperature,
  # and taking it as one printed "95.0 C" for an idle phone. Threshold zones are not
  # measurements.
  case "$t" in *trip*|*limit*|*crit*|*max*|*min*|*gov*) continue ;; esac
  case "$t" in *cpu*) ;; *) continue ;; esac
  [ "${v:-0}" -le 0 ] && continue; [ "${v:-0}" -ge 120000 ] && continue
  [ "$v" -gt "$m" ] && m=$v
done
echo "$m"' 2>/dev/null | tr -d '\r')
  read_skin=$($ADB shell 'm=0
for z in /sys/class/thermal/thermal_zone*; do
  t=$(cat $z/type 2>/dev/null); v=$(cat $z/temp 2>/dev/null)
  case "$t" in *trip*|*limit*|*crit*|*max*|*min*|*gov*) continue ;; esac
  case "$t" in *skin*|*rear*|*front*|*side*) ;; *) continue ;; esac
  [ "${v:-0}" -le 0 ] && continue; [ "${v:-0}" -ge 120000 ] && continue
  [ "$v" -gt "$m" ] && m=$v
done
echo "$m"' 2>/dev/null | tr -d '\r')
  screen=$($ADB shell dumpsys power 2>/dev/null | sed -n 's/^  mWakefulness=\([A-Za-z]*\).*/\1/p' | head -1)
  if $ADB shell dumpsys activity services dev.nekomario.amb 2>/dev/null | grep -q SessionService; then
    active=capturing
  else
    active=idle
  fi
  printf '%s %s %s %s %s %s %s %s\n' \
    "${level:-?}" "${counter:-0}" "${source:-?}" "${temp:-0}" "${read_cpu:-0}" "${read_skin:-0}" "${screen:-?}" "$active"
}

# dumpsys battery temperature is tenths of a degree; thermal zones are milli-degrees.
battery_c() { awk -v v="$1" 'BEGIN { printf "%.1f", v/10 }'; }
zone_c() { awk -v v="$1" 'BEGIN { printf "%.1f", v/1000 }'; }
# Charge counter is microamp-hours; the difference over a run is the charge moved.
mah() { awk -v v="$1" 'BEGIN { printf "%.1f", v/1000 }'; }

# True when adb can still answer for this device: a sample that returns `?` means the
# phone left the bus, and the table must say that rather than print zeros that read like
# a battery that stopped.
reachable() { $ADB shell true >/dev/null 2>&1; }

# Frames the host has decoded so far, from the newest stats line it wrote.
#
# Counting stats *lines* was the first version and it was wrong: the worker emits one per
# second, so "lines per second" is ~1.0 while a healthy stream runs at 30 fps — the table
# said `fps=1.0`, which reads as a catastrophic stall. The frame count is inside the line;
# take it from there.
host_frames() {
  grep -o '"decoded":[0-9]*' "$LOG" 2>/dev/null | tail -1 | cut -d: -f2 || echo 0
}

row() { # label, sample, fps for this interval ("-" when it is the first row)
  printf '%-9s %-6s %-9s %-4s %-10s %-9s %-9s %-9s %-8s %s\n' \
    "$1" "$(echo "$2" | cut -d' ' -f1)%" "$(mah "$(echo "$2" | cut -d' ' -f2)") mAh" \
    "$(echo "$2" | cut -d' ' -f3)" "$(battery_c "$(echo "$2" | cut -d' ' -f4)")C" \
    "$(zone_c "$(echo "$2" | cut -d' ' -f5)")C" "$(zone_c "$(echo "$2" | cut -d' ' -f6)")C" \
    "$(echo "$2" | cut -d' ' -f7)" "$3" "$(echo "$2" | cut -d' ' -f8)"
}

# Decoded frames per second over this interval. This is the number that answers "is video
# still arriving", so it belongs on every row; `-` means no reading yet.
interval_fps() {
  [ "$2" = "" ] && { echo '-'; return; }
  awk -v now="$1" -v before="$2" -v seconds="$3" 'BEGIN{printf "%.1f", (seconds>0) ? (now-before)/seconds : 0}'
}

header() {
  printf '%-9s %-6s %-9s %-4s %-10s %-9s %-9s %-9s %-8s %s\n' \
    elapsed level charge src battery cpu skin screen host state
}

echo "device: $($ADB shell getprop ro.product.model 2>/dev/null | tr -d '\r')"
echo "idle baseline, 30 s..."
before=$(sample)
sleep 30
after=$(sample)
header
row "idle" "$after" '-'
# The control: if the phone is on a charger, battery drain measures the charger and not
# the workload, and the run must say so rather than publish a number that cannot move.
if [ "$(echo "$after" | cut -d' ' -f3)" != "battery" ]; then
  echo "NOTE: the phone is on $(echo "$after" | cut -d' ' -f3) power. Battery drain cannot be"
  echo "      measured from a charger; unplug it for an energy number. Thermal and"
  echo "      throughput numbers below are still about the workload."
fi

LOG=$(mktemp)
echo "starting the host; events in $LOG"
host_args=(--headless --seconds "$SECONDS_TO_RUN")
[ -n "$PHONE_HOST" ] && host_args+=(--host "$PHONE_HOST")
"$HOST_BIN" "${host_args[@]}" >"$LOG" 2>&1 &
HOST_PID=$!

# Capture consent is a human act, so this waits for it instead of pretending to press
# it: the host connects and authorizes by itself, and the phone must then be showing
# カメラ開始 / 配信中. The wait is bounded and says what it is waiting for.
echo "waiting up to 120 s for the phone to be capturing (press カメラ開始 on it)..."
waited=0
while [ "$waited" -lt 120 ]; do
  if $ADB shell dumpsys activity services dev.nekomario.amb 2>/dev/null | grep -q SessionService; then
    break
  fi
  kill -0 "$HOST_PID" 2>/dev/null || break
  sleep 5
  waited=$((waited + 5))
done
if ! $ADB shell dumpsys activity services dev.nekomario.amb 2>/dev/null | grep -q SessionService; then
  echo "the phone never started capturing; the numbers below would be an idle phone." >&2
  echo "host said:" >&2
  tail -4 "$LOG" >&2
  kill "$HOST_PID" 2>/dev/null
  exit 3
fi
echo "capturing; sampling for ${SECONDS_TO_RUN}s"
if [ "$SCREEN_OFF" = "1" ]; then
  # One power press. A `KeyEvent.KEYCODE_POWER` send needs no more privilege than any
  # other input event, and the sample below reports mWakefulness so the table shows
  # whether the screen actually went off rather than assuming the press worked.
  $ADB shell input keyevent 26 >/dev/null 2>&1
  sleep 3
  echo "screen-off requested; wakefulness is now: $($ADB shell dumpsys power 2>/dev/null | sed -n 's/^  mWakefulness=\([A-Za-z]*\).*/\1/p' | head -1)"
fi
echo
header
last_stats=$(host_frames)
last_elapsed=0
row "start" "$(sample)" '-'
started=$(date +%s)
while kill -0 "$HOST_PID" 2>/dev/null; do
  sleep 60
  kill -0 "$HOST_PID" 2>/dev/null || break
  elapsed=$(( $(date +%s) - started ))
  if ! reachable; then
    printf '%-9s %s\n' "${elapsed}s" "phone unreachable (left adb/USB); sampling stops here"
    break
  fi
  now_stats=$(host_frames)
  fps=$(interval_fps "$now_stats" "$last_stats" "$((elapsed - last_elapsed))")
  last_stats=$now_stats
  last_elapsed=$elapsed
  row "${elapsed}s" "$(sample)" "$fps"
done
wait "$HOST_PID"
host_exit=$?

echo
echo "host exit=$host_exit (0 streamed, 1 did not, 2 could not start)"
frames=$(grep -c '"event":"stats"' "$LOG" 2>/dev/null || echo 0)
echo "stats events: $frames"
tail -3 "$LOG"
if [ "$host_exit" != "0" ] || [ "$frames" = "0" ]; then
  echo "NO STREAM: the phone and the host never exchanged video; the numbers above are an idle phone." >&2
  exit 1
fi
