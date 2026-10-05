# 05 — Windows camera and bounded stop

Question: is the Rust worker compatible with the pinned Unity Capture filter and existing termination behavior?

- Seam: Windows `output` and process supervisor; share the video owner from 04. No filter rewrite or Media Foundation camera migration.
- Differential IPC harness: Rust producer -> existing pinned C++ receiver, and existing producer -> Rust receiver fixture, using unregistered slot 40. Verify header offsets, object names, stride units, bottom-up RGBA, event ordering, max size, readiness, skipped frames, 2-second mutex behavior and abandoned mutex cleanup.
- Preserve the exact producer mutex across old and new processes so they cannot race the same camera. Make the direct Rust IPC implementation a measured decision; if a tiny C ABI bridge is necessary, record the failing case and keep one implementation, not two live backends.
- With no consumer, keep decoding but skip conversion/output as in the baseline. Report decoded and submitted counts separately. Exercise Windows Job Object kill-on-close, stalled worker/pipe, parent termination, resource release and the next start; do not inherit the parent's job handle into the worker.
- Visible artifact: pixel-correct IPC fixture, a native consumer receiving the registered camera, and an unresponsive worker that still allows Stop/Close and a later Start.
- Accept: IPC/stall/duplicate producer tests; software and available D3D11VA/CUDA hardware path evidence; native Windows camera/consumer behavior. Wine can support the IPC/installer tests but cannot accept this hardware boundary.
- Visual gate: compare camera output orientation/corners with compare-screenshots and finish with unprimed screenshot-critique.
- Delegated: windows/windows-sys feature subset and native compiler wiring after slice 01. Do not change camera CLSID or registration ownership checks to simplify the port.

Dependency: 04. Next: 06 and full integration.
