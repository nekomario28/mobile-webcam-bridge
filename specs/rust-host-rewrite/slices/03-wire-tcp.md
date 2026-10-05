# 03 — AMB1 and TCP compatibility

Question: does the Rust receiver speak the released Android protocol byte-for-byte?

- Seam: pure AMB1 parser/encoder and TCP Transport; private headless worker harness, no decoder/camera required.
- Freeze baseline encode/decode fixtures from host/tests/wire_test.cpp and Android Wire/VideoWire. Preserve sequence unsigned bits, all flags, bounds and exact message shapes. Do not treat control-interleaved sequence gaps as missing video.
- Visible artifact: a deterministic loopback peer's HELLO/ACK, fragmented frames, PING/PONG and malformed-input outcomes; optional real Android connection only as separately labeled evidence.
- Accept: split header/payload reads, interrupted partial IO, deadline exhaustion, bad magic/version/length, EOF, wrong ACK sequence and non-AMB peer cases have deterministic results; no unbounded allocation. Waiting for camera, transport failure, and reception are distinguishable.
- Worker supervision must pass Stop within the existing 1-second termination bound, pipe EOF, forced parent death, child reaping and a subsequent start. Prove late observations cannot affect the next generation. Linux parent-death signaling must be initialized by the child and checked against the expected parent; do not place process creation in a short-lived thread.
- Delegated: internal parser buffer organization and error wording; preserve real timeouts rather than weakening tests to pass. Existing hostnames/address behavior remains.
- Must preserve: TCP port 48527 and existing Android APK; no remote camera-start command or new authentication/encryption protocol in this slice.

Dependency: 01. Next: 04 uses the same wire/session entry.
