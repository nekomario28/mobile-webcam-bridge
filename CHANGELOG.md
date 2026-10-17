# Changelog

## Unreleased — per-PC authorization (independent review returned a merge blocker)

- Produce the Linux virtual camera through the V4L2 streaming API instead of
  `write(2)`: request two MMAP output buffers, fill them with a monotonic
  timestamp and `TIMESTAMP_COPY`, and drop a frame when no consumer has released
  a buffer. A `write(2)`-only producer cannot serve consumers at all (a reader's
  `STREAMON` fails with `EIO`), and a queued buffer without a timestamp is never
  exposed to a reader either; both were reproduced on a real `v4l2loopback`
  device, where the streaming producer feeds an `ffmpeg` reader 49 frames.
  `exclusive_caps=1` and the shipped device configuration are correct.

- Extend the device acceptance to a mid-stream quality switch on a real Camera2
  camera: PREPARE, exactly one pause barrier, COMMIT with the next epoch, an
  applied result, a new VIDEO_CONFIG and keyframe for the target mode, then live
  access units — followed by revocation, which still stops capture and releases
  the camera. The protocol doc now states that a Commit with a stale epoch is
  ignored without a reply, which this execution reproduced.

- Close the review's UI-wiring guard gap: the authorization producer stop is now
  `CaptureCancellation`, a production class that owns its own daemon thread and
  cannot be pointed at the main looper. `MainActivity` holds the single instance,
  the device acceptance constructs the same class, and two guards (a contract
  test for the threading plus a wiring test over `MainActivity.kt`) were both
  observed failing on a throwaway commit that restored the pre-repair
  `handler.post` stop.

- Record the independently reviewed authorization checkpoint: one Medium blocker,
  AUTHREV-001, because Camera2/MediaCodec teardown still depends on the main
  thread rather than the authorization lifetime, so capture can outlive expiry or
  revocation until the UI thread runs. Output remains gated, but the required
  non-UI producer-cancellation path is not implemented. Merge is blocked until it
  is, with a guard that fails on the old state.
- Record AUTHREV-002: persistence holds the shared authorization monitor, so a
  stalled Keystore/commit call delays expiry and owner close. The owner
  reproduction failed closed and is documented as delayed teardown, not a bypass.
  See docs/reviews/per-pc-authorization-review.md for scope, evidence and limits.
- Second independent review of the repair found that moving persistence off the
owner monitor had made revocation stop taking effect until the write returned
(REV-001) and a cancellation window while the camera session was constructed
(REV-003). Revocation now takes effect in memory before the durable write, and
`start`/`close` of the camera bridge are serialised; both keep a regression, and
REV-001's guard was observed failing on the pre-repair behaviour. Remaining
review items (UI-wiring guard gap, unbounded platform calls on the teardown
thread, MediaCodec call sites, an Allow that persists while its request expires)
are recorded as accepted limitations in the repair review record.
- Repair both findings. The session now takes a required capture-cancellation
  sink, stops the Camera2/MediaCodec producer off the UI thread and refuses to
  install repeating capture when authorization already ended. Authorization
  persistence runs outside the owner monitor with wait-free published snapshots,
  so a stalled Keystore/commit call no longer delays expiry or teardown. Each
  repair keeps a regression that was observed failing on the pre-repair
  behaviour; the Android teardown itself still needs device confirmation and has
  not been independently re-reviewed.

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
- Close an unstarted LAN session's socket during shutdown and refuse restarting
  that closed connection; retain a real-socket EOF/restart regression.
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
