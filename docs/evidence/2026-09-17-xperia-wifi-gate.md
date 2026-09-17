# Xperia Wi-Fi gate — 2026-09-17

Historical record: this run predates v0.1.1 and used the old PIN-based Wi-Fi
handshake. Current releases do not use a PIN.

## Scope

This record covers the real Android-to-Linux Wi-Fi path on the Xperia XQ-GE44
using Mobile Webcam source HEAD `4ea145745b7ecf5beb6f21592c5c5c3df30b62b8`.

The debug APK was built from this checkout. APK SHA-256:
`2ce6cc670b09c42360c912a5f028856f6d381314341d96c4969d3362394138ca`.

The six-digit pairing PIN was supplied interactively and is intentionally not
stored in this repository.

## Result

- Phone Wi-Fi endpoint discovered from the real LAN: `192.168.1.12:48527`.
- Linux connected to the Android TCP server with the supplied PIN.
- Wrong-PIN probe was rejected and the server remained available.
- Linux sink reported `transport=lan` and `decoder=vaapi`.
- Android sent `VIDEO_CONFIG 1280x720 @30 H.264 Annex-B`.
- Linux opened `/dev/video10` as YUYV `1280x720 @30`.
- 60 decoded/output frames completed in `1.98272` seconds.
- Discontinuities: `0`.
- Terminal result: `[PASS G3 V4L2 slice]`.

## Evidence boundary

This proves real phone-to-PC Wi-Fi pairing, the AMB1 media stream, VAAPI
initialization/use for this 60-frame slice, and V4L2 output. It does not prove
long-duration throughput, reconnect behavior, firewall/router compatibility,
or consumer compatibility with OBS, Chromium, Firefox, or Discord.
