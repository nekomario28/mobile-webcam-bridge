#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this installer with sudo." >&2
    exit 1
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
legacy_config=/etc/modprobe.d/nexora.conf
legacy_line='options v4l2loopback video_nr=10 card_label="Nexora Virtual Camera" exclusive_caps=1'
legacy_amb_config=/etc/modprobe.d/android-media-bridge.conf
legacy_amb_line='options v4l2loopback video_nr=10 card_label="Android Media Bridge" exclusive_caps=1'

if [ -e "$legacy_config" ]; then
    if [ "$(cat "$legacy_config")" != "$legacy_line" ]; then
        echo "Refusing to replace modified $legacy_config" >&2
        exit 1
    fi
    rm "$legacy_config"
fi

if [ -e "$legacy_amb_config" ]; then
    if [ "$(cat "$legacy_amb_config")" != "$legacy_amb_line" ]; then
        echo "Refusing to replace modified $legacy_amb_config" >&2
        exit 1
    fi
    rm "$legacy_amb_config"
fi

install -Dm0644 \
    "$script_dir/70-mobile-webcam.rules" \
    /etc/udev/rules.d/70-mobile-webcam.rules
install -Dm0644 \
    "$script_dir/mobile-webcam-v4l2loopback.conf" \
    /etc/modprobe.d/mobile-webcam.conf

udevadm control --reload-rules
udevadm trigger --subsystem-match=usb --attr-match=idVendor=18d1
udevadm trigger --subsystem-match=video4linux --sysname-match=video10

echo "Installed Mobile Webcam USB and v4l2loopback configuration."
echo "Reload v4l2loopback after camera applications release /dev/video10:"
echo "  sudo modprobe -r v4l2loopback"
echo "  sudo modprobe v4l2loopback"
