# Quality control wire contract (AMB1 v0)

Status: wire contract and both-language fixtures implemented; the switching
state machine (Android capture ownership, Rust worker epochs) is slice 02's
remaining work.

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
  2 applied), `u8 reason` (0 none, 1 permission, 2 foreground,
  3 unsupported-mode, 4 stale-revision), `u16 appliedWidth`,
  `u16 appliedHeight`, `u16 appliedFps`, `u16 reserved` (14 bytes).
- `PAUSED` / `COMMIT_QUALITY`: `u32 requestId`, `u32 streamEpoch` (8 bytes).

## Limits

Control payloads are capped at 16 KiB; the mode list at 256 entries and one
in-flight apply. An over-limit list is never silently truncated: the encoder
refuses it and the phone answers with a feature-unavailable capability
notification, disabling only the quality controls while legacy streaming
continues. Payload length is validated against the count before the list is
allocated.
