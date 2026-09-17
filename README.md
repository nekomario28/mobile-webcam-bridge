# Mobile Webcam

Use an Android phone as a low-latency Linux webcam over USB or local Wi-Fi.

```text
Android Camera2
  -> MediaCodec H.264 Surface encoder
  -> AMB1 framed stream
       -> USB / Android Open Accessory
       -> TCP / Wi-Fi + 6-digit pairing PIN
  -> Linux FFmpeg decode
       -> auto hardware decode (VAAPI first, then CUDA)
       -> software fallback when no supported device is available
  -> live rotate / horizontal flip / vertical flip
  -> v4l2loopback (/dev/videoX)
```

The public project name is **Mobile Webcam**. The internal `AMB1` wire magic and
`amb-*` helper names are retained as protocol/implementation identifiers so the
working media path does not need an unnecessary wire-format migration.

## Current state

Implemented:

- Android Camera2 -> MediaCodec input-Surface H.264 producer.
- Bounded latest-live video queue with IDR/config recovery after drops.
- USB AOA transport that does not require USB debugging at runtime.
- Wi-Fi/LAN TCP transport using the same AMB1 framing and producer path.
- One active LAN client with a generated six-digit pairing PIN.
- Linux USB or TCP receiver feeding the same decoder/V4L2 path.
- FFmpeg hardware decode selection: `auto`, `vaapi`, `cuda`, or software.
- Qt 6 desktop GUI for transport selection, Wi-Fi IP/PIN, decode mode,
  `/dev/videoX`, rotation, and horizontal/vertical flip.
- Rotation and flips update while streaming; camera consumers do not need to be
  reopened.
- Linux AppImage build/release path.

Fresh host verification on 2026-09-17:

- Release GUI build succeeds.
- `ctest`: 4/4 PASS (`wire`, `yuyv`, `image-transform`, `tcp`).
- The TCP test covers valid PIN handshake, wrong-PIN rejection, framed PING,
  and an IDR request over localhost.
- Real Xperia Wi-Fi test passes: `192.168.1.12:48527` pairing, H.264 stream,
  VAAPI decode, and 60 V4L2 frames with zero discontinuities.
- On the current Radeon Linux host, `--hw-decode auto` initializes `vaapi`
  before the intentionally unreachable LAN endpoint fails.

Android compilation and the new Wi-Fi path are verified on this checkout. The
long duration gate and iOS sender remain future work. The old USB evidence under
`docs/evidence/` is retained as predecessor evidence; it is not presented as
proof of this exact repository state.

## Why this architecture

The camera and encoder do not care which transport is active. Both USB and
Wi-Fi implement the same small session contract and send the same versioned
frames. That keeps the expensive Camera2/MediaCodec work, backpressure policy,
IDR recovery, decoder, transforms, and V4L2 output shared.

This boundary is also the useful extension point for a future iOS sender. iOS
will need its own capture/encode and USB implementation, but a LAN sender can
target the same framed H.264 session without changing the Linux webcam path.

## Build on CachyOS / Arch

Host dependencies:

```fish
sudo pacman -S --needed base-devel cmake pkgconf libusb ffmpeg qt6-base \
    v4l2loopback-dkms v4l2loopback-utils
```

Build and test:

```fish
cmake -S host -B build/host -DCMAKE_BUILD_TYPE=Release -DAMB_BUILD_GUI=ON
cmake --build build/host --parallel
ctest --test-dir build/host --output-on-failure
```

Install the USB access rule and persistent V4L2 camera label:

```fish
sudo ./linux/install-host-integration.sh
```

After camera applications release `/dev/video10`, reload v4l2loopback once if
the label changed:

```fish
sudo modprobe -r v4l2loopback
sudo modprobe v4l2loopback
cat /sys/class/video4linux/video10/name
```

The expected label is `Mobile Webcam`.

## Wi-Fi usage

1. Open the Android app and press **Connect Wi-Fi**.
2. The phone shows one or more local IPv4 endpoints and a six-digit PIN.
3. Open **Mobile Webcam** on Linux, select **Wi-Fi / LAN**, and enter the phone
   IP and PIN.
4. Keep decode mode at **Auto** unless you are diagnosing a backend.
5. Press **Start bridge**, then start the camera on the phone.

The CLI equivalent is:

```fish
./build/host/amb-v4l2-sink \
  --lan 192.168.1.42 \
  --pin 123456 \
  --device /dev/video10 \
  --hw-decode auto
```

The PIN prevents an accidental peer on the local network from taking the
single receiver slot. The current TCP transport is not encrypted, so use it on
a trusted LAN. Encryption can be added at the transport boundary later without
changing the media session or V4L2 path.

## USB usage

List devices without sending AOA vendor requests:

```fish
./build/host/amb-aoa-probe --list
```

If the phone is still in its manufacturer USB mode, select it in the GUI and
press **Switch to AOA**, or use the CLI with its exact VID:PID:

```fish
sudo ./build/host/amb-aoa-probe --device 0fce:XXXX --switch
```

With USB debugging disabled, the normal accessory-only target is
`18d1:2d00`. Once the Android app opens the accessory, the same Linux sink and
V4L2 output path are used as for Wi-Fi.

## Live orientation controls

The desktop GUI provides 0°, 90°, 180°, and 270° clockwise rotation plus
independent horizontal and vertical flips. Changes are written to the running
sink over its control stdin and apply on subsequent frames without restarting
the bridge or the camera consumer.

Quarter turns are fitted into the existing V4L2 frame size with black bars so
applications already holding `/dev/videoX` do not need a format reopen.

## Hardware decode behavior

`amb-v4l2-sink --hw-decode auto` tries Linux VAAPI first, then CUDA, and uses
software decode when no compatible hardware device can be initialized.
Explicit `vaapi` or `cuda` requests fail instead of silently switching backend,
which keeps diagnostics clear.

The current Radeon host exposes `/dev/dri/renderD*`, FFmpeg lists VAAPI, and the
project decoder reports `decoder=vaapi` during the local initialization probe.
The real Xperia Wi-Fi test also produced 60 frames through this VAAPI path. A
long duration throughput and reconnect test is still required before claiming
production-level network robustness.

## AppImage

```fish
./linux/build-appimage.sh
./dist/Mobile_Webcam-x86_64.AppImage
```

Versioned output is written as
`dist/Mobile_Webcam-<version>-x86_64.AppImage` with a SHA-256 file beside it.
Release signing and GitHub release setup are documented in `docs/RELEASE.md`.

## Remaining device gates

The shortest next device pass is:

1. Repeat USB accessory-only with the new Mobile Webcam AOA identity.
2. Run a longer Wi-Fi throughput and reconnect test.
3. Run OBS/Chromium/Firefox/Discord against `/dev/videoX` only after the stream
   itself is stable.

See `docs/architecture.md`, `docs/gates.md`, and `docs/provenance.md` for the
protocol/evidence boundaries.

## License

Repository-owned source, scripts, and documentation are MIT licensed. libusb,
FFmpeg, Qt, and v4l2loopback retain their upstream licenses. The AppImage
distribution boundary is described in `THIRD_PARTY_NOTICES.md` and
`linux/APPIMAGE_LICENSES.md`.

No GPL/AGPL research-reference source is copied into this repository.
