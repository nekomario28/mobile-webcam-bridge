# Validation gates

## G0 — AOA capability and transition

Preconditions:

- USB debugging OFF.
- No ADB dependency in the host path.
- Data-capable USB cable.
- Mobile Webcam APK already installed by ordinary sideloading.

PASS requires:

1. Host reads non-zero AOA protocol using request 51 while the phone is still
   in its manufacturer USB mode.
2. Host sends zero-terminated manufacturer/model/description/version/URI/serial
   strings using request 52.
3. Host sends request 53.
4. Phone disconnects/re-enumerates as Google VID `0x18d1` with an accessory PID.
5. With USB debugging OFF, target PID is normally `0x2d00`.
6. Android obtains accessory permission and `UsbManager.openAccessory()` succeeds.

Capture exact stdout and `lsusb` before/after as evidence. Prefer selecting the
pre-AOA device explicitly with `--device VID:PID`; do not spray vendor requests
across unrelated USB peripherals when a selector is available.

## G0.5 — framed bulk smoke test

PASS requires the Android app to open `UsbAccessory` and exchange **1000**
deterministic AMB `PING`/`PONG` frames over the accessory bulk endpoints:

- sequence matches on every exchange;
- opaque 8-byte challenge is echoed exactly;
- no malformed header / oversized payload / timeout / disconnect;
- host prints same-clock RTT min/avg/p50/p95/max.

Transport boundary: host → Android sends the fixed 24-byte AMB header and any
non-empty payload as separate USB bulk transfers. Android's accessory stream
API drops unread bytes from a USB transfer when a read consumes only part of
that transfer, while the Android parser intentionally reads header then payload.
Do not coalesce both pieces into one host bulk transfer.

No cross-device latency claim is allowed at this gate.

## G1 — 720p30

- H.264, 1280x720, 30 fps.
- 10 continuous minutes.
- decoder corruption = 0.
- unexpected disconnects = 0.
- queue depth remains bounded.

## G2 — 1080p30

- 1920x1080, 30 fps.
- 30 continuous minutes.
- unexpected disconnects = 0.
- visible corruption = 0.
- no monotonic latency growth.

## G3 — Linux consumer compatibility

Validate the virtual camera in at least:

- OBS;
- Chromium/WebRTC camera picker;
- Firefox/WebRTC camera picker;
- Discord (native or browser path available on the test system).

## G4 — 1080p60

Only after G0-G3 are green. Profile camera, encoder, USB throughput, decoder,
pixel conversion, and sink independently before optimization.
