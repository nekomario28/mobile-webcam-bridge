# Xperia USB-debugging-off host access — 2026-09-16

Status: **accessory-only host access, G0.5, and a short camera-to-V4L2 slice
passed.**

- Xperia XQ-GE44 changed from `0fce:020d` to accessory-only `18d1:2d00`
  after USB debugging was disabled.
- The installed `70-android-media-bridge.rules` assigned the USB node to
  group `video`, mode `0660`, and an ACL granting the active user read/write
  access.
- The non-root host probe connected to `18d1:2d00` and completed 1000/1000
  framed PING/PONG exchanges with zero timeout or mismatch; RTT p95 was
  0.441 ms.
- After reloading v4l2loopback, `/dev/video10` reported the card name
  `Android Media Bridge` and was recreated as `root:video` mode `0660` with
  an active-user read/write ACL.
- After the Android UI started G1, the non-root Linux sink received H.264
  1280x720 at 30 fps from `18d1:2d00`, decoded it to YUYV, and wrote 300 frames
  to `/dev/video10` over 9.91 seconds. The sink reported one discontinuity and
  no timeout or disconnect during the bounded run.

This closes the Linux permission difference between accessory-only `2d00` and
ADB-enabled `2d01` and proves the short camera-to-V4L2 path without USB
debugging. It does not satisfy the formal 10-minute, zero-discontinuity G1 gate
or the full G3 application matrix.
