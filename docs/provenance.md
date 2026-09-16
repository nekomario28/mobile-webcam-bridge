# Research provenance and clean-room boundaries

Date established: 2026-09-06.

## PhoneBridge — `Alivelyyy/PhoneBridge`

Observed reference commit: `0f9c2be451249661cf1675726d775f4deec98d15`.
License: MIT.

Useful evidence/reference:

- Android AOA app + Linux libusb architecture.
- H.264 / v4l2loopback / PipeWire project decomposition.
- Protocol, validation and latency-tracking ideas.

Known reason not to use it as the repository base:

- the observed Linux AOA switch path filtered on the AOA/Google VID before
  attempting transition and sent an incomplete transition sequence; the new
  project implements the AOA procedure from official Android documentation.
- the observed Android video path used ImageAnalysis + CPU YUV repacking into
  MediaCodec input buffers; this project targets Camera2 -> MediaCodec Surface.

Initial bootstrap status: **no PhoneBridge source copied**.

## BrokieCam — `FOSSforBrokies/BrokieCam`

License observed: GPLv2.

Mechanism reference only:

- hardware H.264 encoding;
- SPS/PPS + IDR recovery discipline;
- bounded/latest-frame queue ideas;
- low-latency socket behavior.

Policy: do not copy/adapt source into this MIT repository.

## Linux Link — `ArvinKavaskov/linux-link`

License observed: GPLv3.

Mechanism reference only:

- discovery and identity;
- reconnect/doctor concepts;
- transport separation;
- virtual-device integration lessons.

Its observed webcam path used JPEG/MJPEG, which is not the target media path.
Policy: do not copy/adapt source into this MIT repository.

## Telescope — `LunarKittyy/Telescope`

License observed: AGPLv3.

Mechanism/reference only:

- physical camera/lens enumeration;
- manual Camera2 controls;
- pairing and device-management UX.

Its observed primary video transport was MJPEG/HTTP, which is not the target
media path. Policy: do not copy/adapt source into this MIT repository.

## Nexora

Negative/runtime evidence from the initial investigation:

- RTSP/RTP-over-UDP behavior produced jitter-buffer exhaustion, missed RTP
  packets and cascading H.264 corruption on the test LAN.
- useful as a regression comparison, not as a code base.

No Nexora source, configuration, license, or product name is part of Mobile
Webcam. The Linux virtual camera configured by this repository is named
`Mobile Webcam`; earlier machine-local `Nexora Virtual Camera` and predecessor
`Android Media Bridge` labels are migration history only.

## Official authority

For AOA behavior, Android/AOSP documentation is authoritative over any of the
projects above. For Camera2 encoder surfaces, Android's CameraDevice/MediaCodec
API contracts are authoritative.
