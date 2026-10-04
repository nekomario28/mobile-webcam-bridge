#!/usr/bin/env python3
"""Collect target runtime licenses, including fonts and missing workspace texts."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess
import tempfile
import urllib.request


def upstream_texts(package, source, cache):
    vcs = json.loads((source / ".cargo_vcs_info.json").read_text())
    sha = vcs["git"]["sha1"]
    match = re.match(r"https://github.com/([^/]+/[^/]+)", package["repository"] or "")
    if not match or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise RuntimeError(f"no pinned license source: {package['name']}")
    repository = match[1].removesuffix(".git")
    destination = cache / repository.replace("/", "-") / sha
    receipt = destination / "SOURCE.json"
    if not receipt.exists():
        url = f"https://api.github.com/repos/{repository}/git/trees/{sha}?recursive=1"
        with urllib.request.urlopen(url, timeout=30) as response:
            tree = json.load(response)
        if tree.get("truncated"):
            raise RuntimeError("truncated upstream license inventory")
        parents = pathlib.PurePosixPath(vcs.get("path_in_vcs", "")).parents
        directories = {"", vcs.get("path_in_vcs", ""), *(str(p) for p in parents if str(p) != ".")}
        files = []
        for entry in tree["tree"]:
            path = pathlib.PurePosixPath(entry["path"])
            parent = "" if str(path.parent) == "." else str(path.parent)
            if entry["type"] == "blob" and parent in directories and path.name.lower().startswith(("license", "licence", "copying", "notice")):
                raw = f"https://raw.githubusercontent.com/{repository}/{sha}/{path}"
                with urllib.request.urlopen(raw, timeout=30) as response:
                    data = response.read(256 * 1024 + 1)
                if len(data) > 256 * 1024:
                    raise RuntimeError(f"oversized license: {raw}")
                data.decode("utf-8")
                destination.mkdir(parents=True, exist_ok=True)
                name = str(path).replace("/", "--")
                (destination / name).write_bytes(data)
                files.append({"file": name, "url": raw, "sha256": hashlib.sha256(data).hexdigest()})
        if not files:
            raise RuntimeError(f"license text missing upstream: {package['name']}")
        receipt.write_text(json.dumps(files, indent=2) + "\n")
    for entry in json.loads(receipt.read_text()):
        if hashlib.sha256((destination / entry["file"]).read_bytes()).hexdigest() != entry["sha256"]:
            raise RuntimeError("cached license checksum mismatch")
    return list(destination.iterdir())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("target")
    parser.add_argument("destination", type=pathlib.Path)
    args = parser.parse_args()
    root = pathlib.Path(__file__).resolve().parents[1]
    manifest = root / "Cargo.toml"
    common = ["--locked", "--manifest-path", str(manifest)]
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", *common, "--format-version", "1", "--filter-platform", args.target]))
    tree = subprocess.check_output(["cargo", "tree", *common, "--target", args.target, "--edges", "normal", "--prefix", "none", "--format", "{p}", "--no-dedupe"], text=True)
    names = {tuple(match.groups()) for line in tree.splitlines() if (match := re.match(r"([\w-]+) v([^ ]+)", line))}
    if args.destination.exists():
        raise RuntimeError("choose an empty license destination")
    args.destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=args.destination.parent) as temporary:
        stage = pathlib.Path(temporary) / "licenses"
        stage.mkdir()
        index = []
        for package in metadata["packages"]:
            if (package["name"], package["version"]) not in names or package["source"] is None:
                continue
            source = pathlib.Path(package["manifest_path"]).parent
            texts = [f for f in source.iterdir() if f.is_file() and f.name.lower().startswith(("license", "licence", "copying", "notice"))]
            if package.get("license_file"):
                texts.append(source / package["license_file"])
            if not texts:
                if package["name"] == "ffmpeg-sys-next":
                    texts.extend((root / "licenses/ffmpeg-sys-next").iterdir())
                else:
                    texts.extend(upstream_texts(package, source, root / "target/source-licenses"))
            if package["name"] == "epaint_default_fonts":
                texts.extend(source / "fonts" / name for name in ("emoji-icon-font-mit-license.txt", "OFL.txt", "Hack-Regular.txt", "UFL.txt"))
            destination = stage / f"{package['name']}-{package['version']}"
            destination.mkdir()
            for text in set(texts):
                shutil.copy2(text, destination / text.name)
            index.append({key: package[key] for key in ("name", "version", "license", "repository")})
        (stage / "INDEX.json").write_text(json.dumps(index, indent=2) + "\n")
        stage.rename(args.destination)
    print(f"Collected {len(index)} runtime dependency license records for {args.target}")


if __name__ == "__main__":
    main()
