# A ten-minute stream attempt on the phone (2026-10-23)

The owner's bar for this item is ten minutes. The run stopped at **6 min 27 s** because
the phone left the USB bus, which is a result about the setup rather than about the
app; what the stream did for those six minutes is below.

Device: Sony XQ-GE44 (Android 15), APK `dev.nekomario.amb` **0.2.0** (versionCode 2000,
debug-signed, installed over 0.1.4 in place), host `mobile-webcam` release build,
headless (`host-rs/tools/phone-soak.sh 600`). Wi-Fi transport, TLS 1.3.

## What the stream did

| | |
|---|---|
| frames decoded and submitted | **11 609**, one stats event per second, 382 of them |
| rate | **30.47 fps** against a 30 fps target |
| resolution | 1280x720 |
| decoder | **vaapi** (hardware) for every sample |
| `submitted == decoded` | every sample — nothing was dropped downstream |
| `active` | true in every sample (a consumer was attached throughout) |
| end | `{"event":"error","message":"Connection reset by peer (os error 104)"}` |

## Thermal, and the numbers we could not take

| elapsed | battery | CPU zone | skin zone |
|---|---|---|---|
| idle baseline | 32.1 C | 38.7 C | 33.6 C |
| start | 32.1 C | 43.8 C | 33.8 C |
| 60 s | 32.3 C | 43.8 C | 35.2 C |
| 190 s | 33.3 C | 45.7 C | 36.3 C |
| 320 s | 34.2 C | **53.1 C** | 38.1 C |
| 385 s | phone unreachable | | |

Battery +2.1 C, CPU +14.4 C, skin +4.5 C over 5 min 20 s of streaming. Nothing near a
thermal limit; the CPU zone climbed fastest and was still climbing when the link went.

**Battery drain was not measured**: the phone reported `AC powered: true` for the whole
run, so any current figure would describe the charger. The instrument says so instead of
publishing one (that note is in `phone-soak.sh`).

## Why it stopped, and what that is worth

The phone disappeared from `adb` entirely (`device not found`) at the same moment the
host reported a connection reset: **the USB link dropped**, not the app, and not the
Wi-Fi session. The phone did not come back on its own within two minutes of polling, so
recovering it needs a physical replug.

The host behaved correctly: it reported the reset, stopped the session, and exited 0
because it had streamed. It did not hang, spin, or leave a half-open session.

Two things to take from this beyond the run:

1. **This is the second time this phone has left the USB bus mid-session** in this
   project (the first was during GUI work, hours earlier). The PC's own diagnostics do
   not distinguish "cable" from "port" from "phone", and nothing here should claim to.
   A bench needs a replug path.
2. **A dropped link is not a dropped app**: the notification/service on the phone was
   gone from `adb` only because `adb` itself was gone. Whether the phone was still
   capturing after the USB drop was not observable from this host.

## What is still not measured

- Nine of the ten minutes, because of the above.
- Screen-off behaviour. The screen stayed `Awake` for the whole run, and whether capture
  survives it turning off still needs its own run (see `docs/performance.md` — the
  screen-on flag only applies while the window is visible).
- Energy, because the phone was charging.
