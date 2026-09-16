# Xperia XQ-GE44 short G1 hardware run — 2026-09-16

Status: **production APK, 61-second G1 transport/decoder slice passed.**
The formal 10-minute G1 gate and production G1 in `18d1:2d00` were not run.

- Android 16; USB debugging ON: `0fce:320d` → `18d1:2d01` (accessory + ADB).
  The APK read back from the phone matched the built APK SHA-256
  `fba1d41a70b5940a8085706a4032d2824c6f5d7cac532eaf5cd2f7400963c12d`.
- After restarting the app, production G0.5: 1000/1000 exchanges; RTT p95 0.461 ms.
- Camera 0: H.264 1280x720 @30, 6 Mbps; the activity displayed
  `KEEP_SCREEN_ON` only while the camera ran.
- With the host receiver waiting before starting the camera: 1830 access units,
  61 keyframes, 0 discontinuities, PTS span 61.0944 s, wall time 61.0826 s.
  `ffmpeg -xerror` decoded the entire stream without errors; `ffprobe`
  counted 1830 decoded 1280x720 H.264 frames. No unexpected USB disconnect.
- Local capture and host log: `build/host-current/g1-0916-production-1min.h264`
  and `build/host-current/g1-0916-production-1min.log` (not checked into Git).

The earlier host stopped on a successful zero-byte bulk IN completion before
the next 24-byte AMB header. The preceding H.264 payload was 40,448 bytes,
exactly 79 × the observed 512-byte bulk endpoint packet size. The host now
consumes empty completions without resetting the existing read timeout. An
instrumented 3000-frame diagnostic run crossed multiple such completions.
