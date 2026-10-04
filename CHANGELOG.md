# Changelog

## 0.1.2 - 2026-10-04

- Add an experimental Windows host and a single Setup.exe that installs the app,
  runtime libraries and Unity Capture virtual camera, with a Start menu uninstaller.
- Remember the desktop Wi-Fi IP, transport, decoder and orientation settings.
- Combine USB accessory switching and receiver startup into the desktop Start
  action, and keep the selected phone through USB re-enumeration.
- Let Android USB camera startup wait for the PC connection; combine camera
  start, stop and cancellation into one button and remember the transport.
- Keep Android connections through screen rotation and put camera information
  behind Details.
- Report waiting and streaming separately; stopping the desktop receiver no
  longer blocks the interface.
- Bundle libusb and the one-time camera-access setup in the Linux AppImage.
  Setup enables boot-time module loading and avoids unloading other cameras.

## 0.1.1 - 2026-09-17

- Use the system language for a simple English/Japanese interface.
- Remove the Wi-Fi PIN; LAN connections now use a minimal AMB1 handshake.

## 0.1.0 - 2026-09-17

- Launch the project publicly as **Mobile Webcam** while retaining AMB1 as the
  internal wire protocol.
- Stream Android Camera2 -> MediaCodec H.264 to Linux over USB AOA or Wi-Fi TCP.
- Pair LAN sessions with a generated six-digit PIN and one active client.
- Reuse bounded latest-live video queuing and IDR/config recovery on both
  transports.
- Add FFmpeg hardware decode selection with VAAPI/CUDA support and automatic
  software fallback when hardware initialization is unavailable.
- Provide a Qt 6 desktop GUI for transport, pairing, decode mode, V4L2 output,
  0°/90°/180°/270° live rotation, and independent horizontal/vertical flips.
- Package the Linux GUI as `Mobile_Webcam-<version>-x86_64.AppImage`.
- Add a localhost TCP integration test covering pairing, framed traffic, IDR
  request, and wrong-PIN rejection.
