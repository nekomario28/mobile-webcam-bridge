#!/usr/bin/env python3
"""Reject oversized/extra-codec packages and record the actual runtime closure."""
import argparse
import json
import pathlib
import re
import subprocess

p = argparse.ArgumentParser()
p.add_argument('platform', choices=['linux', 'windows'])
p.add_argument('stage', type=pathlib.Path)
p.add_argument('--objdump', default='x86_64-w64-mingw32-objdump')
a = p.parse_args()
files = [f for f in a.stage.rglob('*') if f.is_file() and not f.is_symlink()]
size = sum(f.stat().st_size for f in files)
if size > 60 * 1024 * 1024:
    raise RuntimeError(f'distribution exceeds 60 MiB: {size}')
for f in files:
    if re.search(r'^(lib)?(Qt[56]|avformat|avdevice|avfilter|swresample)', f.name, re.I):
        raise RuntimeError(f'unexpected runtime library: {f}')
runtimes = []
floor = (0, 0)
for f in files:
    with f.open('rb') as source:
        magic = source.read(4)
    if a.platform == 'linux' and magic == b'\x7fELF':
        info = subprocess.check_output(['readelf', '--version-info', str(f)], text=True)
        required = re.findall(r'Name: GLIBC_(\d+)\.(\d+)', info)
        version = max((tuple(map(int, v)) for v in required), default=(0, 0))
        floor = max(floor, version)
        runtimes.append({'file': str(f.relative_to(a.stage)), 'bytes': f.stat().st_size, 'glibc': '.'.join(map(str, version))})
    elif a.platform == 'windows' and f.suffix.lower() in ('.dll', '.exe'):
        info = subprocess.check_output([a.objdump, '-p', str(f)], text=True)
        runtimes.append({'file': str(f.relative_to(a.stage)), 'bytes': f.stat().st_size, 'imports': re.findall(r'DLL Name: (\S+)', info)})
if a.platform == 'linux' and floor > (2, 39):
    raise RuntimeError(f'glibc requirement exceeds 2.39: {floor}')
receipt = {'expanded_bytes': size, 'glibc_floor': '.'.join(map(str, floor)) if a.platform == 'linux' else None, 'runtime_files': runtimes}
(a.stage / 'runtime-manifest.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt))
