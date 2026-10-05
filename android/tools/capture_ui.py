"""Capture real Android widgets; emulator evidence does not verify phone USB."""

import hashlib
from pathlib import Path
import re
import subprocess
import time
import urllib.request
import xml.etree.ElementTree as ET

OUTPUT = Path("build/android-ui")
PACKAGE = "dev.nekomario.amb"


def adb(*args):
    return subprocess.check_output(["adb", *args], timeout=30)


def capture(name):
    (OUTPUT / f"{name}.png").write_bytes(adb("exec-out", "screencap", "-p"))
    adb("shell", "uiautomator", "dump", "/sdcard/window.xml")
    xml = adb("exec-out", "cat", "/sdcard/window.xml")
    (OUTPUT / f"{name}.xml").write_bytes(xml)
    return ET.fromstring(xml)


def tap(tree, label):
    for node in tree.iter("node"):
        if node.get("text", "").casefold() == label.casefold():
            x1, y1, x2, y2 = map(int, re.findall(r"\d+", node.attrib["bounds"]))
            adb("shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
            time.sleep(1)
            return
    raise RuntimeError(f"Control not found: {label}")


baseline = OUTPUT / "baseline-0.1.2.apk"
with urllib.request.urlopen(
    "https://github.com/nekomario28/mobile-webcam-bridge/releases/download/v0.1.2/Mobile_Webcam-0.1.2-android.apk",
    timeout=30,
) as response:
    baseline.write_bytes(response.read())
assert hashlib.sha256(baseline.read_bytes()).hexdigest() == (
    "f2930db9ed02c4aaaf583617c24acb373b146d3746f5f2755fb23f35fa869526"
)

for version, apk in [
    ("before", baseline),
    ("after", Path("android/app/build/outputs/apk/debug/app-debug.apk")),
]:
    adb("install", str(apk))
    for locale in ("en-US", "ja-JP"):
        adb("shell", "cmd", "locale", "set-app-locales", PACKAGE, "--locales", locale)
        adb("shell", "am", "force-stop", PACKAGE)
        adb("shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
        time.sleep(2)
        tree = capture(f"{version}-{locale}-initial")
        tap(tree, "USB")
        tree = capture(f"{version}-{locale}-usb")
        tap(tree, "Wi-Fi")
        tree = capture(f"{version}-{locale}-wifi")
        if version == "after":
            text = " ".join(n.get("text", "") for n in tree.iter("node"))
            assert "スマホのIPアドレス" in text if locale == "ja-JP" else "Phone IP" in text
    adb("uninstall", PACKAGE)
print("Captured baseline/candidate Android widgets in EN/JA; phone USB is NOT RUN.")
