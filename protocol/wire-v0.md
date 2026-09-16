# AMB wire framing v0 (provisional)

G0 uses only standard Android Open Accessory control requests. G0.5 begins use
of this framing for a real bidirectional bulk-transfer smoke test. The same
framing is intentionally carried forward into the first video slice.

All integer fields are little-endian unless stated otherwise.

```text
u32 magic        = 0x31424d41  # bytes: "AMB1"
u8  version      = 1
u8  type
u16 flags
u32 sequence
u32 payload_len
u64 pts_us
payload[payload_len]
```

Header size: 24 bytes. Current absolute payload allocation limit: 16 MiB.
Production message-specific limits will be tighter.

Initial message types:

- `0x01 HELLO`
- `0x02 HELLO_ACK`
- `0x03 PING` — G0.5, payload is an opaque 8-byte challenge
- `0x04 PONG` — G0.5, same sequence / PTS / payload as PING
- `0x10 VIDEO_CONFIG`
- `0x11 VIDEO_AU`
- `0x12 VIDEO_IDR_REQUEST` — empty payload; request a fresh recovery point
- `0x20 STATS`
- `0x7f ERROR`

## VIDEO_CONFIG v0

Payload is exactly 16 bytes:

```text
u16 width
u16 height
u16 fps
u16 reserved      = 0
u32 bitrate_bps
u8  codec         = 1  # H.264/AVC
u8  nal_format    = 1  # Annex-B
u16 reserved2     = 0
```

The active camera size is reported, not merely the requested size.

## VIDEO_AU v0

Payload is one H.264 access unit normalized to **Annex-B**. MediaCodec output
that is four-byte length-prefixed is converted before it enters the transport.
The host must not infer vendor-specific MediaCodec framing.

Flags:

- bit 0 (`0x0001`): keyframe/IDR;
- bit 1 (`0x0002`): codec config included;
- bit 2 (`0x0004`): discontinuity/recovery boundary.

Every keyframe may be prefixed with the latest Annex-B SPS/PPS and then sets
`CONFIG_INCLUDED`. This is intentional small redundancy so a receiver can join
or recover without depending on stale decoder state.

If the bounded live-video queue overflows, Android clears queued inter-frames,
suppresses further non-IDR access units, requests a fresh IDR, and marks the
next sent keyframe `DISCONTINUITY`. It is invalid to drop an arbitrary H.264
P-frame and then continue forwarding dependent P-frames as if nothing happened.

`pts_us` for VIDEO_AU is the MediaCodec presentation timestamp. Cross-device
latency must not be computed from it until an explicit clock mapping exists.

## Stream discipline

Both sides must use read-exact loops for the fixed header and then the declared
payload. A USB bulk transfer boundary is not a message boundary. Never allocate
from `payload_len` before validating the hard limit and the per-message limit.

G0.5 deliberately uses synchronous PING/PONG so round-trip integrity and host
same-clock RTT can be measured without claiming cross-device clock sync.
