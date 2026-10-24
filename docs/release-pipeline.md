# Release pipeline (Rust product line)

GitHub Actions workflow: `.github/workflows/release.yml`.

## Tags

- Cut `v$(cat VERSION)` where VERSION is `0.2.x`, `0.3.x`, or later.
- Do **not** use this workflow for classic Qt `v0.1.*` tags.

## Linux AppImage

- Built by `linux/build-rust-appimage.sh` → `host-rs/tools/package_linux.sh`.
- Uses the minimal LGPL FFmpeg from `host-rs/tools/build_ffmpeg.py`.
- `linux/build-appimage.sh` is the **classic Qt** path and refuses to run unless
  `AMB_ALLOW_CLASSIC_APPIMAGE=1`. It must not feed GitHub Releases for current tags.

## Android

- Signed `assembleRelease` APK on the self-hosted release runner.

## Windows

- Separate `windows` job on `windows-2025` uploads Setup.exe onto the same release.

## LGPL

- Ship `docs/licenses/FFMPEG_SOURCE_OFFER.md` (when present) next to binaries.
- See `THIRD_PARTY_NOTICES.md`.
