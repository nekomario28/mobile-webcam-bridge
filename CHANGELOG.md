# Changelog

## Unreleased

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
