#!/usr/bin/env bash
# Interactive *physical* reattach acceptance helper for Linux sysfs.
# Human observation + same-port disappearance/reappearance are required.
set -euo pipefail
umask 077
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
program="$repo_root/host-rs/target/debug/mobile-webcam"
sysfs="${MWB_USB_SYSFS_ROOT:-/sys/bus/usb/devices}"
runs=5
selector=""
output_parent="${TMPDIR:-/tmp}"
usage() {
  cat <<'HELP'
Usage: usb-physical-reattach-gate.sh --device VID:PID [--bin PATH] [--runs N] [--out DIR]
Linux only. Starts each USB probe AFTER an operator unplugs and replugs the
phone on the SAME USB port. Uses the physical sysfs port to observe absence
and reappearance, including when VID:PID changes between AOA and phone mode.
This script never resets Android, the host identity, or USB configuration.
A real operator must confirm both physical actions for every attempt.
HELP
}
while (( $# )); do
  case "$1" in
    --device|--bin|--runs|--out)
      (( $# >= 2 )) || { echo "missing value for $1" >&2; exit 2; }
      option="$1"; value="$2"; shift 2
      case "$option" in
        --device) selector="$value" ;;
        --bin) program="$value" ;;
        --runs) runs="$value" ;;
        --out) output_parent="$value" ;;
      esac ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[[ "$selector" =~ ^[[:xdigit:]]{4}:[[:xdigit:]]{4}$ ]] || { echo "--device must be a measured 4-digit hex VID:PID" >&2; exit 2; }
[[ "$runs" =~ ^[1-9]$|^1[0-9]$|^20$ ]] || { echo "--runs must be 1..20" >&2; exit 2; }
[[ -x "$program" && -f "$program" ]] || { echo "host binary not executable: $program" >&2; exit 2; }
[[ -d "$sysfs" ]] || { echo "USB sysfs directory missing" >&2; exit 2; }
selector="${selector,,}"
# Exactly one USB device matching the operator's initial VID:PID, never the
# first device. Resolve once to the physical port; its directory persists as
# the stable identifier across AOA vendor/product changes.
ports=()
for entry in "$sysfs"/*; do
  [[ -f "$entry/idVendor" && -f "$entry/idProduct" ]] || continue
  vid="$(<"$entry/idVendor")"; pid="$(<"$entry/idProduct")"
  if [[ "${vid,,}:${pid,,}" == "$selector" ]]; then ports+=("$entry"); fi
done
(( ${#ports[@]} == 1 )) || { echo "expected exactly one initial VID:PID device; found ${#ports[@]}" >&2; exit 2; }
port="${ports[0]}"
port_name="${port##*/}"
[[ "$port_name" =~ ^[0-9]+-[0-9]+(\.[0-9]+)*$ ]] || { echo "unexpected USB port identifier" >&2; exit 2; }
mkdir -p -- "$output_parent"
evidence_dir="$(mktemp -d "$output_parent/mwb-usb-physical.XXXXXXXX")"
summary="$evidence_dir/summary.tsv"
mode=HARDWARE
[[ "$sysfs" == /sys/bus/usb/devices ]] || mode=SYNTHETIC
{
  printf 'evidence_mode=%s\nusb_port=%s\nfirst_vid_pid=%s\n' "$mode" "$port_name" "$selector"
  printf 'utc_start=%s\n' "$(date -u +%FT%TZ)"
  printf 'checkout_git_sha=%s\n' "$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo unknown)"
  if command -v sha256sum >/dev/null 2>&1; then
    hash="$(sha256sum -- "$program")"; printf 'host_executable_sha256=%s\n' "${hash%% *}"
  else
    printf 'host_executable_sha256=NOT_RECORDED\n'
  fi
  printf 'phone_model=NOT_RECORDED\nandroid_build=NOT_RECORDED\napk_version=NOT_RECORDED\n'
} > "$evidence_dir/metadata.txt"
printf 'iteration\tutc_start\tutc_end\tdetached\treattached\tpreprobe_vid_pid\texit_code\tverdict\n' > "$summary"
printf 'Evidence: %s\n' "$evidence_dir"
[[ "$mode" == HARDWARE ]] || printf 'SYNTHETIC sysfs: this run is NOT hardware evidence.\n'
failures=0
for (( run=1; run<=runs; run++ )); do
  printf -v n '%02d' "$run"
  start="$(date -u +%FT%TZ)"
  printf 'UNPLUG %s/%s port=%s: physically unplug phone, then press Enter\n' "$run" "$runs" "$port_name"
  if ! IFS= read -r _; then echo "operator input ended before unplug confirmation" >&2; exit 1; fi
  if [[ -e "$port" ]]; then
    printf '%s\t%s\t%s\tNO\tNO\tNA\tNA\tFAIL\n' "$n" "$start" "$(date -u +%FT%TZ)" >> "$summary"
    echo "FAIL: USB port still present; no verified detachment" >&2
    exit 1
  fi
  printf 'REPLUG %s/%s port=%s: physically replug SAME port, then press Enter\n' "$run" "$runs" "$port_name"
  if ! IFS= read -r _; then echo "operator input ended before replug confirmation" >&2; exit 1; fi
  if [[ ! -f "$port/idVendor" || ! -f "$port/idProduct" ]]; then
    printf '%s\t%s\t%s\tYES\tNO\tNA\tNA\tFAIL\n' "$n" "$start" "$(date -u +%FT%TZ)" >> "$summary"
    echo "FAIL: same USB port did not reappear" >&2
    exit 1
  fi
  vid="$(<"$port/idVendor")"; pid="$(<"$port/idProduct")"
  current="${vid,,}:${pid,,}"
  [[ "$current" =~ ^[[:xdigit:]]{4}:[[:xdigit:]]{4}$ ]] || { echo "invalid replugged VID:PID" >&2; exit 1; }
  status=0
  log="$evidence_dir/run-$n.log"
  "$program" --usb-probe --frames 1000 --rate 32 --budget-ms 2000 --device "$current" > "$log" 2>&1 || status=$?
  verdict=FAIL
  if (( status == 0 )) && [[ $(grep -Ec '^sent 1000 echoed 1000 sequence-mismatch 0 payload-mismatch 0 timed-out 0 skipped [0-9]+$' "$log") == 1 ]] && [[ $(grep -Fxc PASS "$log") == 1 ]] && ! grep -Fxq FAIL "$log"; then
    verdict=PASS
  else
    failures=$((failures+1))
  fi
  printf '%s\t%s\t%s\tYES\tYES\t%s\t%s\t%s\n' "$n" "$start" "$(date -u +%FT%TZ)" "$current" "$status" "$verdict" >> "$summary"
  printf 'Run %s/%s: %s (exit %s)\n' "$run" "$runs" "$verdict" "$status"
done
printf 'utc_end=%s\n' "$(date -u +%FT%TZ)" >> "$evidence_dir/metadata.txt"
printf 'Total: %s/%s PASS; details: %s\n' "$((runs-failures))" "$runs" "$summary"
(( failures == 0 ))
