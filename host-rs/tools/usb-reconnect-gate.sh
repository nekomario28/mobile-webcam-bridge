#!/usr/bin/env bash
# Repeat G0/G0.5 in fresh host processes without touching app/PC approvals or ADB.
# Physical reattachment, device metadata and release APK checks remain manual gates.
set -euo pipefail
umask 077

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
program="$repo_root/host-rs/target/debug/mobile-webcam"
runs=5
initial_device=""
output_parent="${TMPDIR:-/tmp}"

usage() {
  cat <<'USAGE'
Usage: host-rs/tools/usb-reconnect-gate.sh [--bin PATH] [--runs N] [--out DIR] [--device VID:PID]

Runs a fresh host process for each G0/G0.5 USB probe, with 1000 round trips,
32 PINGs/s and a 2000 ms exchange budget. The phone must already be running
Mobile Webcam; do not close or reset the phone between runs.

  --bin PATH  Host executable (default: host-rs/target/debug/mobile-webcam)
  --runs N    Number of host restarts, 1..20 (default: 5)
  --out DIR   Directory in which to create a private evidence subdirectory
  --device VID:PID  Explicit initial phone ID (4 hex digits each) before AOA.
                    Used ONLY for attempt 1; later attempts auto-select AOA.

No app data, PC identity, USB configuration or phone permissions are changed.
A PASS here is a same-cable host-process test, not a physical reattach gate.
USAGE
}

while (( $# > 0 )); do
  case "$1" in
    --bin|--runs|--out|--device)
      if (( $# < 2 )); then echo "missing value for $1" >&2; exit 2; fi
      option="$1"; value="$2"; shift 2
      case "$option" in
        --bin) program="$value" ;;
        --runs) runs="$value" ;;
        --out) output_parent="$value" ;;
        --device) initial_device="$value" ;;
      esac ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

if [[ ! "$runs" =~ ^[0-9]+$ ]] || (( 10#$runs < 1 || 10#$runs > 20 )); then
  echo "--runs must be an integer from 1 to 20" >&2; exit 2
fi
runs=$((10#$runs))
if [[ -n "$initial_device" && ! "$initial_device" =~ ^[[:xdigit:]]{4}:[[:xdigit:]]{4}$ ]]; then
  echo "--device must be a 4-digit hex VID:PID" >&2; exit 2
fi
if [[ ! -f "$program" || ! -x "$program" ]]; then
  echo "host binary is not executable: $program" >&2; exit 2
fi
mkdir -p -- "$output_parent"
evidence_dir="$(mktemp -d "$output_parent/mwb-usb-reconnect.XXXXXXXX")"
summary="$evidence_dir/summary.tsv"
printf 'iteration\tutc_start\tutc_end\texit_code\tverdict\n' > "$summary"
{
  printf 'utc_start=%s\n' "$(date -u +%FT%TZ)"
  # Checkout SHA is not proof that this particular executable came from it.
  printf 'checkout_git_sha=%s\n' "$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo unknown)"
  if command -v sha256sum >/dev/null 2>&1; then
    executable_hash="$(sha256sum -- "$program")"
    executable_hash="${executable_hash%% *}"
  elif command -v shasum >/dev/null 2>&1; then
    executable_hash="$(shasum -a 256 -- "$program")"
    executable_hash="${executable_hash%% *}"
  else
    executable_hash=NOT_RECORDED
  fi
  printf 'host_executable_sha256=%s\n' "$executable_hash"
  printf 'first_attempt_device_selector=%s\n' "${initial_device:-AUTO_AOA}"
  printf 'runs=%s\nframes=1000\nrate_per_second=32\nbudget_ms=2000\n' "$runs"
  printf 'phone_model=NOT_RECORDED\nandroid_build=NOT_RECORDED\napk_version=NOT_RECORDED\n'
} > "$evidence_dir/metadata.txt"

printf 'Evidence: %s\n' "$evidence_dir"
failures=0
for (( run=1; run<=runs; run++ )); do
  printf -v run_id '%02d' "$run"
  log="$evidence_dir/run-$run_id.log"
  utc_run_start="$(date -u +%FT%TZ)"
  status=0
  args=(--usb-probe --frames 1000 --rate 32 --budget-ms 2000)
  # VID:PID changes when AOA mode is entered; only the first invocation
  # can reliably use the pre-AOA selector. Later runs must find one AOA phone.
  if (( run == 1 )) && [[ -n "$initial_device" ]]; then
    args+=(--device "$initial_device")
  fi
  "$program" "${args[@]}" > "$log" 2>&1 || status=$?
  verdict=FAIL
  if (( status == 0 )) && grep -Eq '^sent 1000 echoed 1000 sequence-mismatch 0 payload-mismatch 0 timed-out 0 skipped [0-9]+$' "$log" && grep -Fxq PASS "$log"; then
    verdict=PASS
  else
    failures=$((failures+1))
  fi
  utc_run_end="$(date -u +%FT%TZ)"
  printf '%s\t%s\t%s\t%s\t%s\n' "$run_id" "$utc_run_start" "$utc_run_end" "$status" "$verdict" >> "$summary"
  printf 'Run %s/%s: %s (exit %s)\n' "$run" "$runs" "$verdict" "$status"
done
printf 'utc_end=%s\n' "$(date -u +%FT%TZ)" >> "$evidence_dir/metadata.txt"
printf 'Total: %s/%s PASS; details: %s\n' "$((runs-failures))" "$runs" "$summary"
(( failures == 0 ))
