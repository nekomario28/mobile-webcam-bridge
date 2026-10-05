"""Capture real Android widgets; emulator evidence does not verify phone USB."""

import hashlib
from pathlib import Path
import re
import socket
import struct
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


def hierarchy():
    adb("shell", "uiautomator", "dump", "/sdcard/window.xml")
    return ET.fromstring(adb("exec-out", "cat", "/sdcard/window.xml"))


def node(tree, resource):
    return next(n for n in tree.iter("node") if n.get("resource-id", "").endswith(":" + resource))


def wait_for_controls():
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        tree = hierarchy()
        if any(n.get("text", "") == "USB" for n in tree.iter("node")):
            return tree
        time.sleep(0.5)
    raise RuntimeError("App controls did not appear")


def tap_node(control):
    x1, y1, x2, y2 = map(int, re.findall(r"\d+", control.attrib["bounds"]))
    adb("shell", "input", "tap", str((x1 + x2) // 2), str((y1 + y2) // 2))
    time.sleep(1)


def tap(tree, label):
    for node in tree.iter("node"):
        if node.get("text", "").casefold() == label.casefold():
            tap_node(node)
            return
    raise RuntimeError(f"Control not found: {label}")


baseline = OUTPUT / "baseline-0.1.3.apk"
with urllib.request.urlopen(
    "https://github.com/nekomario28/mobile-webcam-bridge/releases/download/v0.2.0-beta.3/Mobile_Webcam-0.1.3-android.apk",
    timeout=30,
) as response:
    baseline.write_bytes(response.read())
assert hashlib.sha256(baseline.read_bytes()).hexdigest() == (
    "97adfec9d481510887b407fc674ccd5c024b983db1d47d560cb32cca6289c3ba"
)

for version, apk in [
    ("before", baseline),
    ("after", Path("android/app/build/outputs/apk/debug/app-debug.apk")),
]:
    adb("install", str(apk))
    if version == "after":
        adb("install", "android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk")
        result = adb("shell", "am", "instrument", "-w", f"{PACKAGE}.test/{PACKAGE}.UsbPermissionInstrumentation")
        (OUTPUT / "usb-permission-instrumentation.log").write_bytes(result)
        assert b"PASS USB permission callback" in result and b"INSTRUMENTATION_CODE: -1" in result, result
    for locale in ("en-US", "ja-JP"):
        adb("shell", "cmd", "locale", "set-app-locales", PACKAGE, "--locales", locale)
        adb("shell", "am", "force-stop", PACKAGE)
        adb("shell", "am", "start", "-n", f"{PACKAGE}/.MainActivity")
        wait_for_controls()
        tree = capture(f"{version}-{locale}-initial")
        tap(tree, "USB")
        tree = capture(f"{version}-{locale}-usb")
        if version == "after":
            assert node(tree, "id/usb_mode").get("selected") == "true"
            assert node(tree, "id/camera_action").get("enabled") == "false"
        tap(tree, "Wi-Fi")
        tree = capture(f"{version}-{locale}-wifi")
        if version == "after":
            text = " ".join(n.get("text", "") for n in tree.iter("node"))
            assert "スマホのIPアドレス" in text if locale == "ja-JP" else "Phone IP" in text
            assert node(tree, "id/lan_mode").get("selected") == "true"
            assert node(tree, "id/usb_mode").get("selected") == "false"
            assert node(tree, "id/camera_action").get("enabled") == "false"
            status = node(tree, "id/connection_status").get("text")
            tap_node(node(tree, "id/camera_action"))
            assert node(hierarchy(), "id/connection_status").get("text") == status
            local_port = int(adb("forward", "tcp:0", "tcp:48527").strip())
            try:
                with socket.create_connection(("127.0.0.1", local_port), timeout=5) as peer:
                    peer.sendall(struct.pack("<4sBBHIIQ", b"AMB1", 1, 1, 0, 1, 0, 0))
                    ack = b""
                    while len(ack) < 24:
                        part = peer.recv(24 - len(ack))
                        assert part, "No HELLO_ACK"
                        ack += part
                    assert struct.unpack("<4sBBHIIQ", ack) == (b"AMB1", 1, 2, 0, 1, 0, 0)
                    time.sleep(1)
                    tree = capture(f"after-{locale}-connected")
                    assert node(tree, "id/camera_action").get("enabled") == "true"
                    if locale == "en-US":
                        tap_node(node(tree, "id/camera_action"))
                        dialog = hierarchy()
                        deny = next(n for n in dialog.iter("node") if n.get("resource-id", "").endswith("/permission_deny_button"))
                        tap_node(deny)
                        time.sleep(2)
                        tree = capture("after-en-US-camera-denied")
                        assert "Camera permission denied" in node(tree, "id/connection_status").get("text", "")
                time.sleep(1)
                tree = capture(f"after-{locale}-disconnected")
                assert node(tree, "id/camera_action").get("enabled") == "false"
                assert "accessory stream closed" not in node(tree, "id/connection_status").get("text", "")
            finally:
                adb("forward", "--remove", f"tcp:{local_port}")
            if locale == "ja-JP":
                adb("shell", "wm", "size", "1280x720")
                adb("shell", "settings", "put", "system", "font_scale", "1.5")
                time.sleep(2)
                wait_for_controls()
                capture("after-ja-JP-landscape-large-text")
                adb("shell", "settings", "put", "system", "font_scale", "1.0")
                adb("shell", "wm", "size", "720x1280")
    adb("uninstall", PACKAGE)
print("PASS Android EN/JA controls, disabled pre-connect action, synthetic TCP connect/disconnect, camera denial, and landscape large-text capture; physical phone USB is NOT RUN.")
