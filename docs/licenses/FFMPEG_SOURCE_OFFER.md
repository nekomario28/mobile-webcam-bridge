# FFmpeg / libusb corresponding-source offer

This project dynamically links a **minimal LGPL** FFmpeg build (no
`--enable-gpl`, no `--enable-nonfree`) and system/bundled **libusb**.
Binary distributors must provide corresponding source and build instructions
so recipients can modify and relink those LGPL libraries.

This file is the project's written offer and recipe. It is intended to ship
next to release binaries (AppImage, Windows Setup, and the GitHub Release).

## Exact FFmpeg versions (pinned)

Built by `host-rs/tools/build_ffmpeg.py`:

| Platform | FFmpeg version | Source archive SHA-256 |
| --- | --- | --- |
| Linux (Rust AppImage / CI) | **7.1.5** | `de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f` |
| Windows (Rust Setup / CI) | **9.0.2** | `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e` |

Download URLs:

- `https://ffmpeg.org/releases/ffmpeg-7.1.5.tar.xz`
- `https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz`

### nv-codec-headers (build-time only)

| Item | Value |
| --- | --- |
| Git revision | `0a6fba9a2820628b8103464f4c8753ee05838baa` |
| tarball SHA-256 | `1d2070546de622fd6074a99d4b283e727988b7c3624ef85f97b88962264314d2` |
| URL | `https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/0a6fba9a2820628b8103464f4c8753ee05838baa` |

## How to reproduce the exact library set

```sh
# Linux prefix (matches CI / AppImage)
python3 host-rs/tools/build_ffmpeg.py linux \
  --prefix /tmp/amb-ffmpeg-linux \
  --cache /tmp/amb-ffmpeg-cache

# Windows cross / MSYS prefix
python3 host-rs/tools/build_ffmpeg.py windows \
  --prefix /tmp/amb-ffmpeg-windows \
  --cache /tmp/amb-ffmpeg-cache \
  --cross-prefix=''   # or x86_64-w64-mingw32- when cross-compiling
```

The script refuses a non-empty `--prefix`, verifies the archive checksums,
configures a shared-only H.264 decoder + swscale + avutil build, and fails if
any unexpected decoder/library is enabled. After install it writes
`build-manifest.json` into the prefix; release packages copy that manifest
beside this offer.

### Configure feature set (both platforms)

Common flags (from `build_ffmpeg.py`):

```
--arch=x86_64 --cpu=x86-64
--disable-autodetect --disable-everything
--disable-programs --disable-doc --disable-debug
--disable-static --enable-shared
--disable-avformat --disable-avdevice --disable-avfilter --disable-swresample
--enable-avcodec --enable-avutil --enable-swscale
--enable-decoder=h264
--enable-ffnvcodec --enable-cuda --enable-nvdec --enable-hwaccel=h264_nvdec
```

Linux adds: `--enable-vaapi --enable-hwaccel=h264_vaapi --disable-xlib`  
Windows adds: `--target-os=mingw32 --enable-cross-compile --enable-d3d11va`
`--enable-hwaccel=h264_d3d11va,h264_d3d11va2`

Expected closure after configure: decoder `h264` only; no avformat/avdevice/
avfilter/swresample.

## libusb

libusb is linked dynamically (Linux AppImage bundles `libusb-1.0.so.0` from the
build host; Windows packages the MinGW `libusb` DLL). Corresponding source for
the packaged SONAME should be obtained from the same distribution that provided
the binary (Debian/Ubuntu package source, or MSYS2 `mingw-w64-*-libusb`).
License text: LGPL-2.1-or-later (`linux/licenses/LGPL-2.1-or-later.txt`).

## Written offer

For at least three years after you last distribute a binary of this project that
links these LGPL libraries, the copyright holders of **this repository's
packaging** will provide, on request, a machine-readable copy of the
corresponding FFmpeg source tree and the exact `build_ffmpeg.py` recipe used for
that release (or a later recipe that produces a compatible shared library set),
at no more than the cost of physically performing source distribution.

Contact: open a GitHub issue or discussion on
`https://github.com/nekomario28/mobile-webcam-bridge` with subject
`LGPL corresponding source request`, naming the release tag.

Prefer downloading the pinned tarballs above and running `build_ffmpeg.py`
yourself; that path does not require contacting us.

## Classic Qt packages (historical)

v0.1.2 AppImage/Setup used **system GPL-enabled FFmpeg** and Qt. Do **not** use
this offer for those binaries; see `linux/APPIMAGE_LICENSES.md` and the
dependency-source archives of that release.
