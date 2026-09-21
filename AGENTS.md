# Agent instructions

Use repository-owned architecture, gates, tests, and evidence as canonical truth. Keep Android capture, AMB1 transport, Linux decode/transform, and V4L2 output boundaries explicit.

Before substantial work:

1. Read `README.md`, `docs/architecture.md`, and `docs/gates.md` for the affected surface.
2. Inspect the relevant records under `docs/evidence/` before repeating an experiment or strengthening a claim.
3. Prefer affected build/tests while iterating; run the repository-native gate(s) required by the changed Android, transport, decoder, GUI, packaging, or host-integration surface before completion claims.
4. Preserve the first concrete failure and fix the smallest cause before broadening scope.
5. Keep LIVE, hardware-specific, reconnect, and long-running claims distinct from compile/unit/static evidence.

Do not claim Wi-Fi authentication/encryption, long-running stability, reconnect behavior, device compatibility, or hardware decode support without corresponding evidence. Treat host-integration scripts, udev/device permissions, kernel-module operations, release publication, and writes to `/dev/video*` as consequential surfaces; preserve their existing explicit operator/system boundaries.

Reuse the existing shared session/decoder/recovery/transform path instead of creating transport-specific duplicate implementations.