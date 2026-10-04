# 04 — Shared video path and Linux camera

Question: can a Rust worker turn the same access units into the same usable Linux webcam frames?

- Seam: decoder -> transform/conversion -> V4L2 sink. No UI video buffer; transport-independent fixture replay enters the same decode function as live AMB1.
- Preserve software/VAAPI/CUDA selection, actual backend reporting, fixed output dimensions, YUYV alignment, letterboxing, rotation and flips. Reuse buffers/scalers; no new encoded queue.
- Preserve VIDEO_CONFIG plus config-included keyframe start, strict PTS after start and DISCONTINUITY flush. Active width/height/fps changes and decoder errors visibly terminate the session as in the baseline. No new host-side drop queue, automatic decoder recovery or camera reopening.
- Pin callback storage for the codec lifetime, keep packet padding/allocation FFmpeg-owned, and test resource cleanup on every failure seam. Frame borrows cannot survive the next decode.
- Visible artifact: asymmetric source/corner fixture and captured V4L2 output for every transform; baseline/Rust comparison uses identical CPU-decoded inputs. Hardware numerical differences must be explicitly scoped rather than hidden by loose tolerances.
- Accept: fixture pixel/geometry parity; frame-write errors and receiver backpressure behave correctly; missing device reports setup action; real v4l2loopback and at least one real consumer work. Full G1/G2/G3 remains in 07.
- Visual gate: compare transformed corner/letterbox crops with compare-screenshots; an unprimed screenshot-critique is the final check on camera-output images. Native output evidence cannot be inferred from screenshots of a mock.
- Delegated: Rust internal buffer structs and ioctl bindings. SIMD/fused conversion/zero-copy changes are deferred until matched profiling identifies a bottleneck.

Dependency: 01/03. Next: 05 shares decoder/transform and adds only the Windows external output boundary.
