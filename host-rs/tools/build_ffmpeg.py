#!/usr/bin/env python3
"""Build the host's three FFmpeg libraries; no programs, containers, or other codecs."""
import argparse, hashlib, json, os, pathlib, subprocess, tarfile, urllib.request

PINS={
    "linux":("7.1.5","de668509caf9e35e3cd162473441fdb29538c6d96ed080292b3cf9e6fc5d558f"),
    "windows":("9.0.2","8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"),
}
NV_REV="0a6fba9a2820628b8103464f4c8753ee05838baa"
NV_SHA="1d2070546de622fd6074a99d4b283e727988b7c3624ef85f97b88962264314d2"

def archive(url,path,checksum):
    if not path.exists():
        with urllib.request.urlopen(url,timeout=60) as source, path.open("wb") as output:
            while chunk:=source.read(1024*1024):output.write(chunk)
    if hashlib.sha256(path.read_bytes()).hexdigest()!=checksum:
        raise RuntimeError(f"source checksum mismatch: {path}")

def run(*args,**kwargs):subprocess.run(args,check=True,**kwargs)

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("platform",choices=PINS)
    parser.add_argument("--prefix",type=pathlib.Path,required=True)
    parser.add_argument("--cache",type=pathlib.Path,required=True)
    parser.add_argument("--cross-prefix",default="x86_64-w64-mingw32-")
    args=parser.parse_args();prefix=args.prefix.resolve();cache=args.cache.resolve();cache.mkdir(parents=True,exist_ok=True)
    if prefix.exists() and any(prefix.iterdir()):raise RuntimeError(f"choose an empty build prefix: {prefix}")
    version,checksum=PINS[args.platform];source_tar=cache/f"ffmpeg-{version}.tar.xz"
    archive(f"https://ffmpeg.org/releases/{source_tar.name}",source_tar,checksum)
    nv_tar=cache/f"{NV_REV}.tar.gz";archive(f"https://codeload.github.com/FFmpeg/nv-codec-headers/tar.gz/{NV_REV}",nv_tar,NV_SHA)
    for tar in (source_tar,nv_tar):
        with tarfile.open(tar) as data:data.extractall(cache,filter="data")
    source=cache/f"ffmpeg-{version}";nv=cache/f"nv-codec-headers-{NV_REV}"
    nv_prefix=cache/f"nv-{args.platform}";run("make","install",f"PREFIX={nv_prefix}",cwd=nv)
    environment=dict(os.environ,PKG_CONFIG_PATH=str(nv_prefix/"lib/pkgconfig")+os.pathsep+os.environ.get("PKG_CONFIG_PATH",""))
    build=cache/f"build-{args.platform}";build.mkdir(exist_ok=True)
    flags=[f"--prefix={prefix}","--arch=x86_64","--cpu=x86-64","--disable-autodetect","--disable-everything",
        "--disable-programs","--disable-doc","--disable-debug","--disable-static","--enable-shared",
        "--disable-avformat","--disable-avdevice","--disable-avfilter","--disable-swresample",
        "--enable-avcodec","--enable-avutil","--enable-swscale","--enable-decoder=h264",
        "--enable-ffnvcodec","--enable-cuda","--enable-nvdec","--enable-hwaccel=h264_nvdec"]
    if args.platform=="linux":flags += ["--enable-vaapi","--enable-hwaccel=h264_vaapi","--disable-xlib"]
    else:flags += ["--target-os=mingw32","--enable-cross-compile",f"--cross-prefix={args.cross_prefix}","--pkg-config=pkg-config",
        "--enable-d3d11va","--enable-hwaccel=h264_d3d11va,h264_d3d11va2"]
    run(str(source/"configure"),*flags,cwd=build,env=environment)
    config=(build/"ffbuild/config.mak").read_text()
    enabled=lambda suffix:sorted(line.split("=")[0].removeprefix("CONFIG_").removesuffix(suffix).lower()
        for line in config.splitlines() if line.startswith("CONFIG_") and line.endswith(suffix+"=yes"))
    decoders=enabled("_DECODER")
    if decoders!=["h264"]:raise RuntimeError(f"unexpected decoder closure: {decoders}")
    expected={"h264_nvdec","h264_vaapi"} if args.platform=="linux" else {"h264_nvdec","h264_d3d11va","h264_d3d11va2"}
    if set(enabled("_HWACCEL"))!=expected:raise RuntimeError(f"unexpected hardware decoder closure: {enabled('_HWACCEL')}")
    for name in ("AVFORMAT","AVDEVICE","AVFILTER","SWRESAMPLE"):
        if f"CONFIG_{name}=yes" in config.splitlines():raise RuntimeError(f"unexpected library: {name}")
    run("make","-j1",cwd=build,env=environment);run("make","install",cwd=build,env=environment)
    manifest={"platform":args.platform,"version":version,"sha256":checksum,"nv_headers":NV_REV,
        "flags":flags,"decoders":decoders,"hwaccels":enabled("_HWACCEL")}
    (prefix/"build-manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
    print(json.dumps(manifest))

if __name__=="__main__":main()
