# 01 — Minimal FFmpeg and build closure

Question: can a Rust decoder keep the baseline software/GPU capabilities while removing the unrelated FFmpeg runtime closure?

- Seam: `host-rs/video` binding setup and a small fixture-decoding harness. Establish one Cargo package, pinned toolchain/lock, and an explicit FFmpeg build allowlist.
- Start from baseline Linux 7.1.5 / Windows 9.0.2 sources and hashes. Resolve released ffmpeg-next/sys compatibility; do not rely on a master-branch version declaration being published.
- Windows starts with the GNU/MinGW ABI matching the current native libraries. Derive Linux minimum glibc and CPU ISA from the baseline packaged ELF; do not silently raise either requirement.
- Disable defaults on the wrapper and upstream automatic library discovery. Keep avcodec/avutil/swscale, H.264 and the required hardware APIs. No audio/format/filter/network libraries unless a native link failure proves an internal requirement.
- Visible artifact: same H.264 fixture decoded by baseline and Rust, plus shipped-file/import/dependency sizes for each OS. Report actual software decode; hardware buildability and actual hardware execution separately.
- Accept: native decoder build/fixture passes; feature/import allowlist has no unrelated codec families; version/source/license provenance is reproducible; preliminary combined GUI/video 60 MiB budget is explicitly measured or marked unresolved.
- Delegated: exact released crate/toolchain pins, platform compiler/linker commands, hardware configure flag spelling. Record these before slice 03 consumes them. An unsupported binding/version stops at this seam and reopens the binding decision; do not add an ad hoc compatibility fork.
- Must preserve: H.264 packets/format, software fallback, explicit hardware failures, existing C++ release untouched.

Dependency: none. Next: 02 and 03 can proceed once required build choices are resolved.
