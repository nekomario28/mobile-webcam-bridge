# Changelog

## Unreleased — per-PC authorization (unreviewed integration work)

- Add real user-scoped PC identity persistence with atomic first-use publication,
  Unix private-file checks and Windows DPAPI ciphertext. Add a validated,
  immutable phone authorization repository and Android Keystore AES-GCM storage.
  Both transport startup paths now consume the stores through one phone owner.
- Encode raw P-256 signatures as minimal DER for the phone verifier; cover
  leading-zero scalars with shared Rust/JVM/Android instrumentation fixtures.
- Reject invisible format controls and line/paragraph separators in labels by
  Unicode scalar, including supplementary tags that UTF-16 character checks missed.
  Reuse the already-locked Unicode category crate on the host.
- Reject malformed identity text without a UTF-8 slicing panic. Preserve corrupt
  stored identities for explicit recovery instead of silently replacing them.
- Require fresh per-PC proof before capabilities, controls or output on USB and
  Wi-Fi. Replace unshipped auth v1 with public-key enrollment and a domain-separated
  v2 transcript bound to transport, one-use nonce, point, label and the pinned
  phone certificate on LAN. No empty-list/bearer-only fallback.
- Bind pending action views to one session, distinguish stale actions from
  storage failures and retain prompt cooldown after disconnect/abandonment.
  Refuse a missing authorization record when its Keystore alias survives.
- Add EN/JA phone approval and saved-PC renewal/revocation, live expiry/revoke
  closure, bounded pending requests and a responsive PC wait/Stop with full ID.
  A grant never starts the camera. USB needs no QR but has no cryptographic phone
  attestation or active-physical-relay guarantee.
- Extend emulator conformance with production-Activity pointer actions, persisted
  approval/renew/revoke effects and narrow/scaled captures. Runtime/visual results
  must be checked at the exact revision; tests do not replace independent review
  or physical USB/camera/native-consumer acceptance. Match Android's transformed
  dialog button text in the pointer driver, and keep Android dimension getters
  out of the shared media boundary so the retained socket case runs in Gradle's
  plain JVM suite rather than only with android-all utilities.
- Upgrade the Android app and Rust host together: older clients, including the
  classic Qt host on USB, do not implement mandatory v2 authorization.

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
