# A ten-minute stream on the phone, with the screen off (2026-10-24)

This answers the question the earlier attempt could not reach and the one the
`FLAG_KEEP_SCREEN_ON` discussion kept circling: **does capture survive the screen turning
off?** Yes, on this device, for ten minutes, at full frame rate.

Device: Sony XQ-GE44 (Android 15), APK `dev.nekomario.amb` **0.2.0**, host release build,
headless. Wi-Fi transport (TLS 1.3), USB only for `adb`. Instrument:
`host-rs/tools/phone-soak.sh 600` with `SCREEN_OFF=1`, which presses power once capture is
confirmed and reports `mWakefulness` so the table shows the screen really went off.

## Result

| | |
|---|---|
| duration | **600 s**, 587 stats events |
| frames | **17 851**, decoded and submitted (equal in every sample) |
| rate | **30.46 fps** average; per-minute samples 29.4 – 32.0 |
| resolution / decoder | 1280x720 / **vaapi** (hardware) in every sample |
| consumer attached | `active: true` in every sample |
| screen | **Dozing** for the whole run |
| end | host exit 0 (it streamed), no error event |

```
elapsed   level  charge    src  battery    cpu       skin      screen    host     state
start     64%    3181.1 mAh AC   32.7C      43.0C     34.4C     Dozing    -        capturing
189s      66%    3253.1 mAh AC   34.1C      46.9C     36.7C     Dozing    29.4     capturing
381s      67%    3309.0 mAh AC   35.6C      48.8C     38.2C     Dozing    30.0     capturing
574s      68%    3385.5 mAh AC   37.2C      50.8C     39.3C     Awake     30.0     capturing
```

Thermal over ~9.5 minutes: battery 32.7 → 37.2 C, CPU zone 43.0 → 50.8 C, skin
34.4 → 39.3 C. A steady climb with no plateau reached, and no thermal throttling visible
in the frame rate (30.0 fps at both ends).

## What this settles

- **Screen-off is not a problem for the stream.** The Android app keeps a foreground
  service with `foregroundServiceType="camera"`, which is exactly the mechanism that
  permits background capture; the earlier ledger correction stands (`FLAG_KEEP_SCREEN_ON`
  only affects the screen while the app's window is visible, so it was never the thing
  deciding this). No change is needed to that flag.
- The `fps` column is the host's own decoded counter, not the rate of log lines. The first
  version of the instrument counted stats lines and printed `fps=1.0` for a healthy
  30 fps stream — a number that reads as a catastrophic stall. Fixed before this run.

## What it does not measure

- **Energy.** The phone reported `AC powered: true` throughout (it was on a charger, not
  on the PC's port), so any current or drain figure would describe the charger. The
  instrument prints that warning instead of a number. Measuring drain needs the phone
  unplugged, with Wi-Fi as the only link.
- **The phone's own CPU cost or battery temperature rise under sustained load** beyond the
  thermal zones above.
- **Longer than ten minutes**: the CPU zone was still climbing at the end, so nothing here
  says what happens at thirty.
- The app's own capture catalog, seen in the same session via
  `quality_capabilities`, tops out at **3840x2160 @ 60 fps (108 Mbps)** — which is worth
  noting next to the host's parser bound of 8192x8192 (Q5 of the review): the bound is
  four times beyond anything this app can offer.
