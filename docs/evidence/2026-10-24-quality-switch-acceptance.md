# Quality switch accepted on a physical phone (2026-10-24)

Gate #8 asks for physical-device verification of the resolution and frame-rate controls.
The driver is `--headless --quality WxH@fps` (this repository), which asks the phone to
change mode **once streaming has started** and exits 1 unless the phone confirms that exact
mode. The phone applies the switch through the real capture path: the worker drives the
staged apply and sends the commit; the CLI only says what to want.

## Results

| requested | device confirmed | exit |
|---|---|---|
| `640x480@30` | `applied 640x480@30: PASS` | 0 |
| `1280x720@15` | `applied 1280x720@15: PASS` | 0 |

Both runs: Sony XQ-GE44 (Android 15), APK built from `main` (versionCode 2000), Wi-Fi
transport (TLS 1.3, the PC approved once), the app streaming, one tap on カメラ開始 per run.

```
mobile-webcam --headless --transport lan --host 192.168.1.15 --seconds 40 --quality 640x480@30
requesting 640x480@30 from the phone
applied 640x480@30: PASS
```

The first changes resolution **and** frame rate, the second keeps the resolution and changes
only the frame rate, so both axes are exercised. Each run also proves the negotiation around
it: a `quality_progress`/`quality_applied` exchange over the same session that carries video,
with the mode confirmed by the phone rather than assumed by the host.

## What this does not claim

- Not the full #8 matrix: two modes out of the catalog the phone advertises (which lists
  ~80 combinations). The two chosen cover both axes; the rest is repetition of the same path.
- No claim about *measured* frame rate or image quality after a switch — the gate is about
  the control path accepting, applying and confirming a mode. Per-mode throughput belongs to
  the throughput and soak measurements.
- Not a Windows run, and not a repeated-run stability figure: two runs, one phone, one host.
