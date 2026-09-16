# Xperia XQ-GE44 G0.5 transfer-boundary diagnosis — 2026-09-15

Status: **G0.5 transport fix validated on an instrumented Android build; G1 remains unproven.**

## Hardware / mode

- Xperia XQ-GE44, Android 16.
- USB debugging disabled before the target run.
- Manufacturer mode observed as `0fce:020d`.
- AOA transition observed as accessory-only `18d1:2d00`.

## Failure before the fix

The host could enter both `18d1:2d01` and `18d1:2d00`, and the Android app
successfully reached `UsbManager.openAccessory()` with its accessory reader
thread running. The first G0.5 request still timed out waiting for PONG.

The failure was below AMB framing semantics. The host emitted a complete
32-byte PING frame (`24-byte header + 8-byte payload`) in one USB bulk transfer,
while the Android peer intentionally read the header and payload separately.
Android's accessory-stream contract drops unread bytes from a transfer when a
read consumes only part of that transfer, so the 8-byte payload was discarded
after the 24-byte header read and the Android reader blocked waiting for bytes
that could no longer arrive.

## Fix candidate

Host `AccessoryDevice::send_frame()` now preserves the same transport boundary
used by the Android writer:

1. send the 24-byte AMB header as one bulk transfer;
2. when present, send the payload as a second bulk transfer.

## Hardware result after the fix

After an electrically equivalent USB reset and a fresh AOA transition:

```text
[PASS G0] bus=1 addr=19 VID=18d1 PID=2d00
Target accessory-only PID 2d00 observed.

[PASS G0.5] exchanges=1 ... rtt_ms min=4.305 avg=4.305 p50=4.305 p95=4.305 max=4.305

[PASS G0.5] exchanges=1000 ...
rtt_ms min=0.323 avg=0.436644 p50=0.427 p95=0.508 max=4.428
```

After the subsequent G1 attempt timed out, a further one-exchange G0.5 probe
still passed at 1.333 ms RTT, confirming that the AOA session itself remained
healthy.

## Evidence boundary

- The Android APK used for this diagnosis was an instrumented temporary build
  with logging, direct-boot support, and a development-only automatic G1 start
  hook. Those changes are not part of the production feature branch.
- The host transport fix was replayed onto the feature branch as
  `ff76f87741b7a056b9ff6829face0354c855e81a` before publication.
- The G1 receiver observed zero video frames and timed out. The phone was still
  in `RUNNING_LOCKED` after the diagnostic reboot, so this run is **not** G1
  evidence and does not establish the cause of the camera-side failure.
