#!/usr/bin/env bash
# Run the device conformance on a physical phone, and restore the phone no matter
# how this exits.
#
# The UI conformance resizes the display (`wm size 840x1400`), switches the app
# locale and changes the font scale, and restores them in a `finally` block. That
# block does not run when the process is killed or the caller is interrupted:
# measured 2026-10-19, an aborted run left `Override size: 840x1400` on a physical
# phone until it was reset by hand. This wrapper therefore restores on EXIT, INT
# and TERM, and it installs the trap before anything can change the display.
#
# It also refuses to change the display at all unless the caller says so, because
# a phone that is being used as a phone should not be resized by a test run.
#
# Usage:
#   android/tools/physical-conformance.sh --serial <adb-serial> --camera
#   android/tools/physical-conformance.sh --serial <adb-serial> --conformance [--tls-client <path>]
#
#   --camera        camera acceptance only (real Camera2/MediaCodec capture,
#                   revocation teardown, a mid-stream switch). Needs CAMERA.
#   --conformance   storage, signatures, Keystore TLS 1.3 and the approval UI.
#                   Without --tls-client it starts the phone side and prints the
#                   two values an external TLS client needs; with it, the client
#                   runs against the forwarded port as the CI job does.
set -uo pipefail

serial=""; mode=""; tls_client=""; # The Gradle module is `app`, so the APKs live one level below `android/`.
apk_dir="$(cd "$(dirname "$0")/../app" && pwd)"
while [ $# -gt 0 ]; do
  case "$1" in
    --serial) serial="${2:?--serial needs a value}"; shift 2 ;;
    --camera) mode=camera; shift ;;
    --conformance) mode=conformance; shift ;;
    --tls-client) tls_client="${2:?--tls-client needs a path}"; shift 2 ;;
    --apk-dir) apk_dir="${2:?--apk-dir needs a path}"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
[ -n "$serial" ] || { echo "--serial is required" >&2; exit 2; }
[ -n "$mode" ] || { echo "one of --camera or --conformance is required" >&2; exit 2; }

export PATH="$PATH:/data/android-sdk/platform-tools"
adb() { command adb -s "$serial" "$@"; }

output_dir="$(mktemp -d)"; echo "logs: $output_dir"
display_touched=false
original_screen_off="$(adb shell settings get system screen_off_timeout 2>/dev/null | tr -d '\r')"
restore_screen_off() {
  case "$original_screen_off" in
    ''|null|*[!0-9]*) return ;;
    *) adb shell settings put system screen_off_timeout "$original_screen_off" >/dev/null 2>&1 ;;
  esac
}
restore() {
  # Always, and before anything else can run: whatever the exit was.
  if [ "$display_touched" = true ]; then
    adb shell wm size reset >/dev/null 2>&1
    adb shell wm density reset >/dev/null 2>&1
    adb shell settings delete system font_scale >/dev/null 2>&1
    adb shell cmd locale set-app-locales dev.nekomario.amb --locales "" >/dev/null 2>&1
    restore_screen_off
    echo "display, font scale, app locale and screen timeout restored"
  fi
  adb forward --remove-all >/dev/null 2>&1
  adb shell am force-stop dev.nekomario.amb >/dev/null 2>&1
}
trap 'restore' EXIT
trap 'echo "interrupted; restoring"; exit 130' INT TERM

app_apk="$apk_dir/build/outputs/apk/debug/app-debug.apk"
test_apk="$apk_dir/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk"
[ -f "$app_apk" ] || { echo "missing $app_apk; run ./gradlew :app:assembleDebug" >&2; exit 2; }
[ -f "$test_apk" ] || { echo "missing $test_apk; run ./gradlew :app:assembleDebugAndroidTest" >&2; exit 2; }

adb install -r "$app_apk" | tail -1
adb install -r "$test_apk" | tail -1
# Both modes need a profile the caller has cleared: the conformance refuses a
# stored approval, and the camera acceptance refuses a non-empty store.
adb shell pm clear dev.nekomario.amb | tail -1
adb shell input keyevent KEYCODE_WAKEUP >/dev/null 2>&1
adb logcat -c

if [ "$mode" = camera ]; then
  adb shell pm grant dev.nekomario.amb android.permission.CAMERA
  adb shell am instrument -w -e physicalDevice true -e cameraAcceptance true \
    dev.nekomario.amb.test/dev.nekomario.amb.UsbPermissionInstrumentation | tee "$output_dir/camera-acceptance.log"
  adb pull /sdcard/Android/data/dev.nekomario.amb/files/camera-acceptance "$output_dir/" >/dev/null 2>&1
  grep -qE 'PASS camera acceptance|SKIP camera acceptance|FAIL:' "$output_dir/camera-acceptance.log" || {
    echo "no verdict marker; that is a failure, not a pass" >&2; exit 1; }
else
  # The conformance expects no camera grant: a prepare must answer with a typed
  # permission rejection rather than starting a camera.
  adb shell pm revoke dev.nekomario.amb android.permission.CAMERA >/dev/null 2>&1 || true
  adb shell settings put system screen_off_timeout 600000 >/dev/null 2>&1
  display_touched=true
  adb forward tcp:48527 tcp:48527 >/dev/null
  adb shell am instrument -w -e physicalDevice true \
    dev.nekomario.amb.test/dev.nekomario.amb.UsbPermissionInstrumentation > "$output_dir/instrumentation.log" 2>&1 &
  instrument_pid=$!
  pin=""; key=""
  for _ in $(seq 1 60); do
    line="$(adb logcat -d -s SecureWifiGate:I 2>/dev/null | grep -oE 'READY SHA256=[A-F0-9]{64} KEY=[0-9a-fA-F]{32}' | tail -1 || true)"
    if [ -n "$line" ]; then pin="${line##*SHA256=}"; pin="${pin%% KEY=*}"; key="${line##*KEY=}"; break; fi
    kill -0 "$instrument_pid" 2>/dev/null || break
    sleep 1
  done
  if [ -n "$pin" ] && [ -n "$tls_client" ]; then
    PHONE_SHA256="$pin" PHONE_KEY="$key" python3 "$tls_client" | tee "$output_dir/tls-client.log"
  elif [ -n "$pin" ]; then
    echo "phone is listening: PHONE_SHA256=$pin PHONE_KEY=$key"
    echo "run the external TLS client against 127.0.0.1:48527 while this command runs"
  fi
  wait "$instrument_pid"; echo "instrumentation exit=$?"
  cat "$output_dir/instrumentation.log"
  adb pull /sdcard/Android/data/dev.nekomario.amb/files/authorization-ui "$output_dir/" >/dev/null 2>&1
fi
echo "artifacts in $output_dir"
