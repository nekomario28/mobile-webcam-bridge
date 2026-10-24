#!/bin/sh
# Host integration setup.
#
#   sudo linux/install-host-integration.sh [--switch-parent PID]
#       Manual use and the classic Qt host. Assets are the files next to this
#       script. pkexec must not be pointed at that path: it is derived from
#       wherever the application binary lives and may be user-writable.
#
#   pkexec --disable-internal-agent /bin/sh -s -- \
#       [--helper PATH] [--switch-parent PID --device-json JSON]
#       The Rust host writes this script to the shell's standard input, with
#       the asset placeholders below already replaced from strings compiled
#       into the binary. $0 is /bin/sh, so nothing is read from a path next
#       to the executable. The device JSON cannot share that standard input;
#       it is passed as an argument and given to the USB helper on a pipe.
set -eu

validate_helper_path() {
    path=$1
    case "$path" in
        /*) ;;
        *) echo "Invalid USB helper path." >&2; exit 1 ;;
    esac
    case "$path" in
        *'
'*) echo "Invalid USB helper path." >&2; exit 1 ;;
    esac
    name=$(basename -- "$path")
    if [ "$name" != "mobile-webcam-usb" ]; then
        echo "Invalid USB helper path." >&2
        exit 1
    fi
    rest=${path#/}
    while [ -n "$rest" ]; do
        component=${rest%%/*}
        if [ -z "$component" ] || [ "$component" = "." ] || [ "$component" = ".." ]; then
            echo "Invalid USB helper path." >&2
            exit 1
        fi
        case "$rest" in
            */*) rest=${rest#*/} ;;
            *) rest= ;;
        esac
    done
    if [ -L "$path" ] || [ ! -f "$path" ]; then
        echo "USB helper is not included in this build." >&2
        exit 1
    fi
}

validate_device_json() {
    json=$1
    if [ -z "$json" ]; then
        echo "Invalid device identity." >&2
        exit 1
    fi
    rest=$json
    while [ -n "$rest" ]; do
        char=${rest%"${rest#?}"}
        rest=${rest#?}
        case "$char" in
            [0-9A-Za-z]|'{'|'}'|'"'|':'|','|'['|']') ;;
            *) echo "Invalid device identity." >&2; exit 1 ;;
        esac
    done
}

reject_unembedded_asset() {
    content=$(cat "$1")
    case "$content" in
        *__MOBILE_WEBCAM_*)
            echo "Host setup assets were not embedded." >&2
            exit 1
            ;;
    esac
}

install_embedded_rules_and_modprobe() {
    rules=$(mktemp)
    modprobe=$(mktemp)
    trap 'rm -f "$rules" "$modprobe"' EXIT
    cat > "$rules" <<'MOBILE_WEBCAM_ASSET_END'
__MOBILE_WEBCAM_UDEV_RULES__
MOBILE_WEBCAM_ASSET_END
    cat > "$modprobe" <<'MOBILE_WEBCAM_ASSET_END'
__MOBILE_WEBCAM_MODPROBE_CONF__
MOBILE_WEBCAM_ASSET_END
    reject_unembedded_asset "$rules"
    reject_unembedded_asset "$modprobe"
    install -Dm0644 "$rules" /etc/udev/rules.d/70-mobile-webcam.rules
    install -Dm0644 "$modprobe" /etc/modprobe.d/mobile-webcam.conf
    rm -f "$rules" "$modprobe"
    trap - EXIT
}

install_embedded_modules() {
    modules=$(mktemp)
    trap 'rm -f "$modules"' EXIT
    cat > "$modules" <<'MOBILE_WEBCAM_ASSET_END'
__MOBILE_WEBCAM_MODULES_CONF__
MOBILE_WEBCAM_ASSET_END
    reject_unembedded_asset "$modules"
    install -Dm0644 "$modules" /etc/modules-load.d/mobile-webcam.conf
    rm -f "$modules"
    trap - EXIT
}

embedded=0
script_dir=
case $(basename -- "$0") in
    install-host-integration.sh)
        script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
        ;;
    *)
        embedded=1
        ;;
esac

switch_parent=
helper=
device_json=
if [ "$embedded" -eq 0 ]; then
    if [ "$#" -ne 0 ]; then
        if [ "$#" -ne 2 ] || [ "$1" != "--switch-parent" ]; then
            echo "Usage: $0 [--switch-parent PID]" >&2
            exit 1
        fi
        case "$2" in
            ''|*[!0-9]*|0) echo "Invalid parent PID." >&2; exit 1 ;;
        esac
        switch_parent=$2
    fi
else
    case "$#" in
        0) ;;
        2)
            if [ "$1" != "--helper" ]; then
                echo "Usage: host setup [--helper PATH] [--switch-parent PID --device-json JSON]" >&2
                exit 1
            fi
            helper=$2
            ;;
        6)
            if [ "$1" != "--helper" ] || [ "$3" != "--switch-parent" ] || [ "$5" != "--device-json" ]; then
                echo "Usage: host setup [--helper PATH] [--switch-parent PID --device-json JSON]" >&2
                exit 1
            fi
            helper=$2
            case "$4" in
                ''|*[!0-9]*|0) echo "Invalid parent PID." >&2; exit 1 ;;
            esac
            switch_parent=$4
            device_json=$6
            ;;
        *)
            echo "Usage: host setup [--helper PATH] [--switch-parent PID --device-json JSON]" >&2
            exit 1
            ;;
    esac
    if [ -n "$helper" ]; then
        validate_helper_path "$helper"
    fi
    if [ -n "$device_json" ]; then
        validate_device_json "$device_json"
    fi
    if [ -n "$switch_parent" ] && [ -z "$helper" ]; then
        echo "USB helper is not included in this build." >&2
        exit 1
    fi
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "Run this installer with sudo." >&2
    exit 1
fi

if [ "$embedded" -eq 0 ] && [ -n "$switch_parent" ] && [ ! -f "$script_dir/mobile-webcam-usb" ]; then
    echo "USB helper is not included in this build." >&2
    exit 1
fi

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

if [ "$embedded" -eq 0 ]; then
    install -Dm0644 \
        "$script_dir/70-mobile-webcam.rules" \
        /etc/udev/rules.d/70-mobile-webcam.rules
    install -Dm0644 \
        "$script_dir/mobile-webcam-v4l2loopback.conf" \
        /etc/modprobe.d/mobile-webcam.conf
else
    install_embedded_rules_and_modprobe
fi

udevadm control --reload-rules
udevadm trigger --subsystem-match=usb --attr-match=idVendor=18d1

if [ "$embedded" -eq 0 ]; then
    install -Dm0644 "$script_dir/mobile-webcam-modules-load.conf" /etc/modules-load.d/mobile-webcam.conf
else
    install_embedded_modules
fi
if [ ! -e /dev/video10 ]; then
    if [ -d /sys/module/v4l2loopback ]; then
        v4l2loopback-ctl add -x 1 -n "Mobile Webcam" /dev/video10
    else
        modprobe v4l2loopback
    fi
fi
helper_src=
if [ "$embedded" -eq 0 ]; then
    if [ -f "$script_dir/mobile-webcam-usb" ]; then
        helper_src=$script_dir/mobile-webcam-usb
    fi
elif [ -n "$helper" ]; then
    validate_helper_path "$helper"
    helper_src=$helper
fi
if [ -n "$helper_src" ]; then
    install -Dm0755 "$helper_src" /usr/lib/mobile-webcam/mobile-webcam-usb
else
    udevadm trigger --subsystem-match=usb --attr-match=idVendor=0fce --attr-match=idProduct=020d
fi
udevadm trigger --subsystem-match=video4linux --sysname-match=video10
udevadm settle
test -e /dev/video10
echo "Mobile Webcam camera access is ready."
if [ -n "$switch_parent" ]; then
    if [ "$embedded" -eq 0 ]; then
        exec /usr/lib/mobile-webcam/mobile-webcam-usb --switch --parent "$switch_parent"
    fi
    printf '%s\n' "$device_json" | /usr/lib/mobile-webcam/mobile-webcam-usb --switch --parent "$switch_parent"
fi
