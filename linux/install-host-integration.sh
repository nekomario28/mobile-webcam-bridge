#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this installer with sudo." >&2
    exit 1
fi

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if ! modinfo v4l2loopback >/dev/null 2>&1; then
    echo "Install your distribution's v4l2loopback package for the running kernel first." >&2
    exit 1
fi
if [ -e /dev/video10 ]; then
    camera_name=$(cat /sys/class/video4linux/video10/name)
    case "$camera_name" in
        "Mobile Webcam"|"Android Media Bridge"|"Nexora Virtual Camera") ;;
        *) echo "/dev/video10 belongs to another camera. Select an existing loopback device in the app." >&2; exit 1 ;;
    esac
elif [ -d /sys/module/v4l2loopback ] && ! command -v v4l2loopback-ctl >/dev/null 2>&1; then
    echo "Install v4l2loopback-ctl to add a camera without unloading other cameras." >&2
    exit 1
fi
for config in /etc/modprobe.d/mobile-webcam.conf /etc/modules-load.d/mobile-webcam.conf; do
    [ -e "$config" ] || continue
    case "$config" in
        /etc/modprobe.d/*) expected='options v4l2loopback video_nr=10 card_label="Mobile Webcam" exclusive_caps=1' ;;
        *) expected='v4l2loopback' ;;
    esac
    if [ "$(cat "$config")" != "$expected" ]; then
        echo "Refusing to replace modified $config" >&2
        exit 1
    fi
done
legacy_config=/etc/modprobe.d/nexora.conf
legacy_line='options v4l2loopback video_nr=10 card_label="Nexora Virtual Camera" exclusive_caps=1'
legacy_amb_config=/etc/modprobe.d/android-media-bridge.conf
legacy_amb_line='options v4l2loopback video_nr=10 card_label="Android Media Bridge" exclusive_caps=1'

if [ -e "$legacy_config" ]; then
    if [ "$(cat "$legacy_config")" != "$legacy_line" ]; then
        echo "Refusing to replace modified $legacy_config" >&2
        exit 1
    fi
fi

if [ -e "$legacy_amb_config" ]; then
    if [ "$(cat "$legacy_amb_config")" != "$legacy_amb_line" ]; then
        echo "Refusing to replace modified $legacy_amb_config" >&2
        exit 1
    fi
fi
rm -f "$legacy_config" "$legacy_amb_config"

install -Dm0644 \
    "$script_dir/70-mobile-webcam.rules" \
    /etc/udev/rules.d/70-mobile-webcam.rules
install -Dm0644 \
    "$script_dir/mobile-webcam-v4l2loopback.conf" \
    /etc/modprobe.d/mobile-webcam.conf

udevadm control --reload-rules
udevadm trigger --subsystem-match=usb --attr-match=idVendor=18d1

install -Dm0644 "$script_dir/mobile-webcam-modules-load.conf" /etc/modules-load.d/mobile-webcam.conf
if [ ! -e /dev/video10 ]; then
    if [ -d /sys/module/v4l2loopback ]; then
        v4l2loopback-ctl add -x 1 -n "Mobile Webcam" /dev/video10
    else
        modprobe v4l2loopback
    fi
fi
udevadm trigger --subsystem-match=usb --attr-match=idVendor=0fce --attr-match=idProduct=020d
udevadm trigger --subsystem-match=video4linux --sysname-match=video10
udevadm settle
test -e /dev/video10
echo "Mobile Webcam camera access is ready."
