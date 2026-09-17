# Architecture

## Data path

```text
Android sender
  Camera2
    -> MediaCodec H.264 input Surface
    -> bounded access-unit queue
    -> MediaSession
         -> AccessorySession -> AOA bulk USB
         -> LanSession       -> TCP / Wi-Fi

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

The Linux sink chooses USB or TCP before entering one common frame/decode loop.
Transport-specific code is limited to connect/read/write behavior.

## Backpressure and recovery

Video is live data, so queue growth must stay bounded. `FramedOutbound` keeps a
small video queue. When the peer cannot keep up, stale queued frames are
dropped, the session enters recovery, and a fresh IDR is requested. The first
recovery keyframe carries codec config and a discontinuity flag.

Control frames are kept separate from the bounded video queue so PONG,
VIDEO_CONFIG, and recovery messages are not trapped behind stale video.

## LAN connection

The Android LAN server listens on TCP port `48527` by default and accepts one
active client. The host sends an empty AMB1 `HELLO`; the phone replies with an
empty `HELLO_ACK` after validating the frame.

There is no PIN or encryption in the current transport. Use it on a trusted
local network. The one-client rule prevents accidental concurrent receivers,
but it is not an access-control boundary.

## Hardware decode

The decoder remains FFmpeg/libavcodec. `HardwareDecode` selects a compatible
FFmpeg hardware configuration and transfers decoded hardware frames back to a
system-memory frame for the existing transform/YUYV stage.

`auto` tries VAAPI then CUDA. If neither hardware device can be initialized,
the decoder remains software. An explicit backend request reports failure so a
diagnostic does not silently test a different path.

## Future iOS path

The portable boundary is `encoded H.264 + AMB1 session`, not Android AOA.
An iOS LAN sender can implement the same framed session while using iOS-native
capture/encode APIs. A future wired iOS transport can be added beside AOA/TCP
without changing the Linux decode/V4L2 contract.

## Evidence boundary

Host unit/integration tests can prove framing, transforms, TCP handshake, and
buildability. They cannot prove Android socket behavior, Camera2/MediaCodec
runtime behavior, Wi-Fi throughput, sustained hardware decode, or real V4L2
application compatibility. Those claims require the corresponding device or
runtime gate.
