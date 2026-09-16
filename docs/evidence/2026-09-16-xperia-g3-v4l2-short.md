# Xperia XQ-GE44 short G3 V4L2 run — 2026-09-16

Status: **Linux decode and v4l2loopback output slice passed.** The full G3
application matrix was not run.

- Xperia Android 16 in `18d1:2d01`, Camera2/MediaCodec H.264 1280x720 @30.
- `amb-v4l2-sink` decoded with libavcodec, converted to YUYV, and wrote 900
  frames to `/dev/video10` over 29.9947 seconds with zero discontinuities.
- While the sink was active, `/dev/video10` advertised Video Capture,
  raw YUYV 1280x720 at 30 fps.
- An independent FFmpeg V4L2 consumer read 60 frames. Its framemd5 output
  reported rawvideo 1280x720 and distinct frame hashes.
- The host also recovered from a receiver handoff by discarding 96 stale USB
  transfers before the next valid 24-byte AMB header.
- With the updated APK, a second sink attached without stopping the camera,
  received the re-sent `VIDEO_CONFIG`, and wrote 90 frames with zero
  discontinuities.

This proves the native Linux consumer and one generic V4L2 reader. OBS,
Chromium, Firefox, Discord, production `18d1:2d00`, and latency remain untested.
