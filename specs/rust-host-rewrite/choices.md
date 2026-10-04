# Implementation choices

| Choice | Audit | Confidence | Reason |
|---|---|---|---|
| Published FFmpeg bindings with narrow RAII | sound | high | Codec-only ffmpeg-next failed on unguarded avformat imports. Published ffmpeg-sys-next builds against both minimal prefixes without vendoring a wrapper or retaining avformat. Real software decode/transform tests pass. |
| Vulkan on Linux and DX12 on Windows within eframe/wgpu | provisional | medium | One renderer, no Qt or unused GPU backend. Linux software-adapter captures establish layout; native GPU, IME/accessibility and Windows GUI acceptance remain open. |
| Anonymous pipes and fresh per-session channels | sound | high | Repeated Start/Stop and abrupt parent-death tests exercise the actual worker. Independent priority Stop, one-second forced termination and asynchronous reaping keep the GUI available. Linux and supplementary Wine ownership tests pass. |
| Reuse existing Android, USB/native output and installers | sound | high | Avoids simultaneous phone protocol, USB library and virtual camera rewrites. Qt settings fixtures and synthetic pinned Unity IPC establish compatibility at their tested boundaries. Hardware parity remains open. |
| Demand-aware Windows conversion | provisional | medium | Preserve decoding and the output mapping, bootstrap the pinned receiver, then skip conversion/submission after one second without Want. Validate fresh/open/close/reopen behavior; this is demand evidence, not proof of frames displayed by DirectShow. |

## Independent review disposition

The first code review found twelve concrete defects. Corrections retain the Want event during fresh receiver initialization; enforce exact pre-switch USB identity and wait for owned re-enumeration; clamp positive USB deadlines to at least one millisecond; tolerate only the initial permission-wait IDR timeout; reap asynchronously; make helper cancellation return Idle; resolve the staged Linux setup path and unwritable output; trim the active output name; reject positive partial V4L2 frame writes; and implement Qt's variable-width escapes. Tests exercise short writes, cancellation, settings controls and fresh receiver IPC. Hardware-specific claims remain provisional.

The initial visual review found an unlabeled decode dropdown and weak inactive transport affordance. The UI now labels decoding and frames both transport choices. Its English-labeled capture was actually run with inherited Japanese LANGUAGE; final captures explicitly bind each locale. Empty space and unequal widths were not treated as functional defects. Final captures require a fresh unprimed review.
