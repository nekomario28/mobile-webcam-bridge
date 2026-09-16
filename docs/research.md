# Research notes — 2026-09-06

## Official Android authority used for G0

AOA 1.0 documents the required transition sequence:

1. request 51 (`GET_PROTOCOL`), 16-bit little-endian protocol result;
2. request 52 for zero-terminated UTF-8 identification strings, IDs 0..5;
3. request 53 (`START_ACCESSORY`);
4. wait for USB re-enumeration;
5. identify accessory mode by Google VID `0x18d1` and accessory PID.

Official reference:
https://source.android.com/docs/core/interaction/accessories/aoa

Android's USB accessory documentation explicitly states that USB debugging is
not required for accessory connections. Runtime communication on Android uses
`UsbManager.openAccessory()` and the accessory bulk endpoints.

Official references:
https://developer.android.com/develop/connectivity/usb
https://developer.android.com/develop/connectivity/usb/accessory

## Camera path authority for G1

Android `CameraDevice` documentation explicitly supports a MediaCodec encoder
Surface as a Camera2 capture-session output: configure the encoder, call
`MediaCodec.createInputSurface()`, and use a size reported by the camera stream
configuration for MediaCodec.

Official reference:
https://developer.android.com/reference/android/hardware/camera2/CameraDevice

This is the basis for choosing Camera2 -> MediaCodec Surface rather than an
ImageAnalysis -> CPU YUV repack -> encoder-input-buffer pipeline.
