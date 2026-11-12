#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cat > "$work/fake-mobile-webcam" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
[[ "$*" == '--usb-probe --frames 1000 --rate 32 --budget-ms 2000' ]]
n=0
[[ ! -f "$FAKE_COUNTER" ]] || n="$(cat "$FAKE_COUNTER")"
n=$((n+1))
printf '%s\n' "$n" > "$FAKE_COUNTER"
printf 'sent 1000 echoed 1000 sequence-mismatch 0 payload-mismatch 0 timed-out 0 skipped 1\n'
if [[ "${FAKE_FAIL_AT:-}" == "$n" ]]; then
  printf 'FAIL\n'
  exit 1
fi
printf 'PASS\n'
FAKE
chmod +x "$work/fake-mobile-webcam"
export FAKE_COUNTER="$work/counter"
"$root/usb-reconnect-gate.sh" --bin "$work/fake-mobile-webcam" --runs 3 --out "$work" > "$work/success.log"
summary="$(find "$work" -name summary.tsv -print -quit)"
[[ -n "$summary" ]]
[[ "$(grep -c $'\tPASS$' "$summary")" -eq 3 ]]
[[ "$(ls "$(dirname "$summary")"/run-*.log | wc -l)" -eq 3 ]]
rm -f "$FAKE_COUNTER"
rm -rf "$(dirname "$summary")"
status=0
FAKE_FAIL_AT=2 "$root/usb-reconnect-gate.sh" --bin "$work/fake-mobile-webcam" --runs 3 --out "$work" > "$work/failure.log" || status=$?
[[ "$status" -eq 1 ]]
summary="$(find "$work" -name summary.tsv -print -quit)"
[[ "$(grep -c $'\tPASS$' "$summary")" -eq 2 ]]
[[ "$(grep -c $'\tFAIL$' "$summary")" -eq 1 ]]
[[ "$(grep -c $'\t' "$summary")" -eq 4 ]]
echo 'PASS usb reconnect gate script success and failure paths'
