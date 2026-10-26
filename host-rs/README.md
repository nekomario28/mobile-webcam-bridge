# Connection and setup

Open Mobile Webcam on your phone and PC. Choose the same mode on both:

- **USB:** connect the cable. A single phone is selected for you; press **Connect**.
  Android asks once whether the app may use the accessory; tick the dialog's
  *always allow* option and the prompt does not come back (the grant is remembered per
  app and accessory, and it survives replugging — it is cleared by reinstalling the app).
  Then allow this PC on the phone and press **Start camera**.
- **Wi-Fi (development build, Android 10+):** the short path is the cable. After the
  PC is allowed over USB, press **Send Wi-Fi pairing to PC** on the phone, tap
  **Wi-Fi** there, and press **Switch to Wi-Fi** on the PC. Nothing is typed.
  Secure Wi-Fi uses TLS 1.3 with certificate pinning. A PC that cannot use the
  cable still uses **Details → Show pairing key**: verify the SHA-256 fingerprint
  on the phone, then enter it, the 128-bit access key, and the phone IP. The old
  C++ host cannot connect to the secure-only phone listener.

In authorization-v2 development builds, match the full PC ID and approve the
request on the phone first. USB needs no QR, but attachment alone grants nothing.
Use matching Android and Rust builds; older hosts cannot pass this gate on either
transport. Approval does not start capture. Press **Start camera** on the phone
and allow camera access. If USB access is
declined, select USB again to retry. USB debugging is not required.

## Platform setup

| Platform | Requirement |
| --- | --- |
| Windows | Run Setup.exe once for the virtual camera; DX12 graphics driver. |
| Linux | Install your distribution's v4l2loopback module; Vulkan driver/loader and glibc 2.39 or newer. |

On Linux, **Connect** requests OS authorization when camera integration is
missing, then resumes connecting. Setup does not install kernel modules.

If the host window shows video but no application can see the camera, check how
the loopback device appears to a reader:

```sh
v4l2-ctl -d /dev/video10 --all   # a reader must see "Video Capture"
```

This repository installs `exclusive_caps=1` (see
`linux/mobile-webcam-v4l2loopback.conf`, and the installer refuses a modified
configuration). The host produces through the V4L2 streaming API for exactly this
reason: with a `write(2)`-only producer a reader cannot even start streaming
(`EIO`), and a queued buffer without a timestamp is never exposed to it either.
Both failure modes and their reproduction are tracked in issue #18.
Windows USB also requires a libusb-compatible phone/accessory driver;
Setup.exe installs the virtual camera only.

**Published v0.2.0 releases use plaintext Wi-Fi.** In this unreleased
security branch, Wi-Fi is TLS 1.3-only and requires the correct pinned
certificate and access key; it does not silently fall back to plaintext.
On Windows the access key is stored with per-user DPAPI protection. On Linux
the settings file uses permissions 0600; secrets are not encrypted at rest.
Treat the key as sensitive, and rotate it
from the phone if exposed (this disconnects Wi-Fi clients but leaves their
per-PC grants intact). Use **Authorized PCs** for individual expiry/renewal/
revocation; see [per-PC authorization](../docs/per-pc-authorization.md).
Physical Android, Linux and Windows end-to-end acceptance is still pending.
See [security and test boundaries](../docs/security.md).

Rotation, flips and advanced settings are in **Details**.

## Running without a window

```
mobile-webcam --headless [--transport usb|lan] [--host ADDRESS] [--seconds N]
```

Starts the same worker the window starts, prints every control event as one JSON line
on stdout, and stops after `N` seconds or when the session ends. A Wi-Fi pairing
event's access key is printed as `<redacted>`. The exit code is the
answer a script gates on: **0** if a stream happened, **1** if it did not, **2** if it
could not start (no settings, unreadable key, worker refused). It reads the stored
settings, so pairing is unchanged, and `--transport`/`--host` override only that run.

## Build and verification

The Rust host reuses the Android app, AMB1 protocol, FFmpeg, libusb and Windows
virtual camera. Build entry points are in [tools/](tools/); dependencies are
pinned in Cargo.lock and the build scripts.

**Capturing the GUI's states without a phone.** `tools/capture-gui-states.sh`
builds `--features capture`, replays each `tests/fixtures/capture/*.json` through
the real session (`MW_CAPTURE_FIXTURE`) and writes one PNG per locale with a
hashed manifest — the native capture entry point, not a second rendering. It also
fails if two fixtures come out indistinguishable, because a state mapping that
regresses to one picture would otherwise pass silently.

Three things have to be right for a headless capture, and each cost an
investigation:

- a display the process can draw to: a dedicated `Xvfb` screen **with a window
  manager**, so the window is mapped where `MW_CAPTURE_POSITION` puts it;
- **`WAYLAND_DISPLAY` cleared**: with it set the toolkit picks Wayland and nothing
  lands on the X display being captured;
- `LANGUAGE` as well as `LC_ALL` for the locale, because the Unix locale provider
  prioritises `LANGUAGE`.

`MW_CAPTURE_DETAILS=1` opens the details section (and asks for a window tall
enough to contain it, so the quality controls are inside the frame).

[Implementation evidence](../specs/rust-host-rewrite/implementation.md) records
CI and outstanding device checks. Linux/Xperia success was reported by the user;
physical USB on Linux/Windows was not retested in this update. Classic Qt
[v0.1.2](../docs/classic-qt.md) remains available for rollback with a matching
older APK, not as a USB bypass for an authorization-v2 phone.
