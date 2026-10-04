# 07 — Complete app, packaging, and parity cutover

Question: does the whole Rust entry point reduce distribution complexity without regressing webcam use?

- Connect the native GUI/controller to its worker mode and existing setup integration. UI state comes only from lifecycle observations; distinguish camera submission from decoded frames and Windows consumer readiness. Keep manual reconnect/address persistence and the existing Android camera-start flow.
- Exercise cancellation during setup/connect, fast Start/Stop/Start, stale child messages, stalled pipes, parent termination, close while waiting, and phone stop/unplug. Logs/control traffic cannot grow with frame rate. No preview or automatic retry/discovery added here.
- Reuse Windows NSIS and Linux AppImage/install integration. Update file ownership/uninstaller manifests to the exact Rust staged closure; retain camera collision/refusal checks, source/license provenance, relocation checks, user-file retention and one-time host setup. Pin VERSION/APK compatibility and build identity.
- An upgrade must remove owned v0.1.2 Qt/runtime/helper files using the verified old inventory or existing uninstaller; the new manifest alone cannot do this. Preserve user files, native settings, unrelated camera registration and both registered filter bitnesses. The installer never migrates settings under an elevated user's namespace.
- Visible artifact: one Windows Setup.exe and one Linux AppImage, both launched from the exact packaged contents; clean install/update from v0.1.2/uninstall with settings retained.
- Accept: the contracts.md matrix, native language checks (cargo fmt, scoped clippy, cargo tests/build), G0/G0.5/G1/G2/G3 and real consumers on both OS/transports. Report available hardware backends and NOT RUN paths. Capture same-input CPU/GPU/RSS/drop/queue observations against baseline; do not derive cross-device latency from raw PTS.
- Reconcile size target after early measurements. Record actual expanded/compressed bytes, shipped library/import lists, crates and user-installed prerequisites; no unrelated Qt/codec closure. Do not lower a gate merely to label a release green.
- Visual gate: compare native package UI and camera-output shots for input/action legibility and orientation, then run unprimed screenshot-critique as the last visual check.
- Cutover: only after acceptance, remove C++ host build targets/Qt GUI/old helper binaries and temporary differential harness production dependencies. Keep protocol fixtures, native settings compatibility and the pinned external filter. Ship no runtime language selector/fallback dispatcher. The old git tag and shared saved settings provide rollback.
- Delegated: release version chosen from the actual accepted delta; native packaging compiler consistent with slice 01; tiny reversible layout adjustments. Publication requires the user's release request, not this design task alone.

Dependencies: 01–06. Initial release complete only when required work and gates have evidence; optional iOS/discovery require new slices.
