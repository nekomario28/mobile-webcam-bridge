# Quality control wire contract (AMB1 v0)

Status: wire, Android command delivery/capture ownership/writer boundary and
both-language fixtures implemented. PC polling and config/keyframe/output-API
confirmation have synthetic-peer and real-FFmpeg/output-sink regressions.
Secure-transport integration and native-device acceptance remain separate gates.

Both languages read the same golden fixture:
`protocol/fixtures/quality-capabilities-v1.bin` — a `QUALITY_CAPABILITIES`
frame with extension version 1, permission ready, catalog revision 7, stream
epoch 42, and three modes (720p30, 1080p30, 720p60). Each language's test
decodes the fixture, asserts the values, and re-encodes to the identical
bytes, so the type numbers, field order and counted list layout cannot drift
between Rust and Kotlin.

## Types

Control space, after the session types; video types (0x10+) untouched.

| Type | Name | Direction |
|------|------|-----------|
| 0x05 | `QUALITY_CAPABILITIES` | phone → PC |
| 0x06 | `PREPARE_QUALITY` | PC → phone |
| 0x07 | `QUALITY_RESULT` | phone → PC |
| 0x08 | `PAUSED` | phone → PC |
| 0x09 | `COMMIT_QUALITY` | PC → phone |

Old peers ignore unknown received types (existing behavior); a new PC sends
no new command before the capability notification arrives. A phone without
the extension keeps serving legacy video; no connection error.

## Payloads (little-endian)

- `QUALITY_CAPABILITIES`: `u8 extensionVersion`, `u8 permission`
  (0 ready, 1 awaiting-permission, 2 feature-unavailable),
  `u16 catalogRevision`, `u32 streamEpoch`, `u16 modeCount`,
  then `modeCount` × (`u16 width`, `u16 height`, `u16 targetFps`,
  `u16 reserved`, `u32 bitrate`).
- `PREPARE_QUALITY`: `u32 requestId`, `u16 catalogRevision`, `u16 width`,
  `u16 height`, `u16 targetFps`, `u16 reserved` (14 bytes).
- `QUALITY_RESULT`: `u32 requestId`, `u8 status` (0 switching, 1 rejected,
  2 capture-started, 3 staged-waiting-for-phone-user), `u8 reason`
  (0 none, 1 permission, 2 foreground, 3 unsupported-mode, 4 stale-revision,
  5 busy, 6 deadline, 7 capture-failed), `u16 appliedWidth`,
  `u16 appliedHeight`, `u16 appliedFps`, `u16 reserved` (14 bytes).
- `PAUSED` / `COMMIT_QUALITY`: `u32 requestId`, `u32 streamEpoch` (8 bytes).

## Limits

Control payloads are capped at 16 KiB; the mode list at 256 entries and one
in-flight apply. An over-limit list is never silently truncated: the encoder
refuses it and the phone answers with a feature-unavailable capability
notification, disabling only the quality controls while legacy streaming
continues. Payload length is validated against the count before the list is
allocated. USB and LAN host-control readers validate the exact type-specific
length before allocation. A duplicate prepare replays only its reply, never
capture actions; a pending request cannot be replaced. A commit must name the
same request and advance the ending epoch by exactly one.

The phone's single outbound writer fences capture callbacks by generation.
Old queued video/config is dropped at pause; an already-writing old frame
finishes before PAUSED. After PAUSED, late old callbacks cannot populate the
new generation, including codec metadata used for keyframe recovery.
A staged reply is also a media fence: the PC may discard a retained old
configuration even if the phone has already stopped its camera. Replaying a
PAUSED/staged reply after a new generation starts replays only the frame; it
must not repeat the fence and disable that generation.

## PC application boundary

Receive polls retain partial AMB1 headers/payloads across TCP/USB timeouts.
A partial frame has one five-second budget, not a renewed timeout per fragment;
expiry or invalid framing is terminal. Idle and configured streams both poll so
Apply, Stop and the monotonic apply deadline remain observable during a stall.

These frames ride whichever transport the session uses. Over Wi-Fi that is
the authenticated TLS 1.3 transport, and the receive polls above are the same
ones that keep a partially received TLS record intact: a poll timeout returns
`Ok(None)` with the AMB1 fragment retained, so neither an AMB1 frame boundary
nor a TLS record is lost by a short poll.

PAUSED must match the request and advertised ending epoch. Before Commit, the
PC releases its output, decoder, conversion buffers and timestamp state. A
matching width/height/target-fps config only begins output confirmation: Applied
requires a CONFIG_INCLUDED keyframe to decode and a converted frame to be
successfully submitted to the output API. A staged idle mode still waits for the
phone user's camera start. Status 3, not the presence of an old PC config,
identifies that wait. The phone emits status 0 when the user starts the staged
camera, arming both the phone start timer and PC output timer before Camera2
setup; status 2 confirms only the capture target, not PC output acceptance.
The desktop parent also supervises pending operations so a stuck native worker
cannot indefinitely retain the UI's apply state. Deadlines are enforced at the
controller's polling cadence, not as a hard real-time OS/hardware guarantee. No output acceptance means no Applied and no automatic
retry or rollback. Output-API submission is not proof of a consumer application's
resolution or measured frame rate; those remain native-output acceptance gates.
