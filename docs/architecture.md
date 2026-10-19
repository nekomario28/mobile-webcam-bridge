# Architecture

## Data path

```text
Android sender
  Camera2
    -> MediaCodec H.264 input Surface
    -> bounded access-unit queue
    -> MediaSession
         -> AccessorySession -> AOA bulk USB
         -> LanSession       -> TLS 1.3 / Wi-Fi

AMB1 framing
  HELLO / HELLO_ACK   LAN protocol handshake
  PING / PONG         control/health
  VIDEO_CONFIG        dimensions/fps/codec contract
  VIDEO_AU            Annex-B H.264 access units
  VIDEO_IDR_REQUEST   decoder/recovery request

Linux receiver
  AccessoryDevice or TcpConnection
    -> shared AMB1 frame loop
    -> FFmpeg H.264 decoder
         -> auto hardware device (VAAPI, then CUDA)
         -> software when auto cannot initialize hardware
    -> transform stage
         -> rotate 0/90/180/270
         -> horizontal flip
         -> vertical flip
    -> YUYV
    -> v4l2loopback
```

## Ownership boundaries

`CameraBridgeController` owns camera/encoder lifecycle. `MediaSession` is the
small transport-facing contract: codec config, active camera config, access
units, IDR request callback, and drop count. USB and LAN therefore reuse the
same encoder, framing, bounded queue, and recovery policy.

The Linux sink chooses USB or LAN before entering one common frame/decode loop.
Every socket transport shares one AMB1 framing and pairing implementation;
transport-specific code is limited to connect and the optional security layer.

**The session owns the foreground while it captures.** `SessionService` is a
foreground service of type `camera`, started when the camera opens and stopped
when it closes, and the Activity no longer stops capture in `onPause`. This is
not a preference: a phone used as a webcam is, by definition, being used for
something else at the same time. Measured on a physical phone (2026-10-19),
pressing Home ended the session — the TCP connection closed and the consumer read
0 frames — because `onPause` tore the camera down and Android stops camera access
for a background app. The ongoing notification is the other half of the contract:
it says the camera is still live and carries the action that stops it, so
continuing to capture is visible rather than silent. What `onPause` still does is
hide the pairing key and clear `FLAG_SECURE`, because those must not survive in
the task switcher.

## Backpressure and recovery

Video is live data, so queue growth must stay bounded. `FramedOutbound` keeps a
small video queue. When the peer cannot keep up, stale queued frames are
dropped, the session enters recovery, and a fresh IDR is requested. The first
recovery keyframe carries codec config and a discontinuity flag.

Control frames are kept separate from the bounded video queue so PONG,
VIDEO_CONFIG, and recovery messages are not trapped behind stale video.

## LAN connection

The Android LAN server listens on TCP port `48527` by default and accepts one
active client. The host completes TLS 1.3 against a certificate fingerprint
pinned from the phone screen, then sends the 128-bit access key inside the
encrypted AMB1 `HELLO`; the phone replies with an empty `HELLO_ACK` only after
validating it. There is no plaintext fallback on the production Wi-Fi path.
See [transport security](security.md) and
[wire-secure-lan](../protocol/wire-secure-lan.md).

## Capture quality control

The phone owns what it can capture: it advertises the size/fps tuples its camera
and the selected encoder actually support, and it owns the first camera start
and the permission decision. The PC owns the request: it resolves the user's
saved wish against the advertised list and sends one concrete tuple, never a
substitute for an unsupported one.

A switch is a fenced transaction on the phone's single outbound writer. The
in-flight old frame may finish, but it is written before `PAUSED`; queued and
late old-generation video and codec metadata cannot follow it. The PC releases
its output, decoder, conversion and timestamp state before `Commit`, and reports
applied only after a matching config, a config-bearing keyframe, a successful
decode and a successful output submission. Metadata is never the confirmation.

Nothing retries or rolls back on its own. A failed apply leaves the previous
stream running or the session failed, with the reason reported; the user decides
whether to try the same wish again. A phone whose camera is stopped stages the
request and waits for its user to start it, and the deadline starts when capture
actually starts rather than when the request was staged.

These frames ride whichever transport the session uses, so over Wi-Fi they are
inside the TLS session above. Receive polls retain partially received AMB1
frames and TLS records alike; a partial frame has one lifetime, after which the
connection terminates rather than trickling indefinitely.

## Hardware decode

The decoder remains FFmpeg/libavcodec. `HardwareDecode` selects a compatible
FFmpeg hardware configuration and transfers decoded hardware frames back to a
system-memory frame for the existing transform/YUYV stage.

`auto` tries VAAPI then CUDA. If neither hardware device can be initialized,
the decoder remains software. An explicit backend request reports failure so a
diagnostic does not silently test a different path.

## Future iOS path

The intended browser route uses Safari camera capture over HTTPS and WebRTC.
It needs a WebRTC receiver that feeds decoded frames into the host transform and
virtual-camera path. Android's raw AMB1-over-TCP session is not exposed to a
browser; USB AOA remains Android-specific.

## Evidence boundary

Host unit/integration tests can prove framing, transforms, TLS and TCP
handshake, and buildability. They cannot prove Android socket behavior, Camera2/MediaCodec
runtime behavior, Wi-Fi throughput, sustained hardware decode, or real V4L2
application compatibility. Those claims require the corresponding device or
runtime gate.
