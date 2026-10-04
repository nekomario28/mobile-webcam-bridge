#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
build_dir="$repo_root/build/appimage"
appdir="$build_dir/AppDir"
tool_dir="$build_dir/tools"
dist_dir="$repo_root/dist"
version=$(cat "$repo_root/VERSION")
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "VERSION must use major.minor.patch" >&2
    exit 2
fi

linuxdeploy="$tool_dir/linuxdeploy-x86_64.AppImage"
linuxdeploy_url="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
linuxdeploy_sha="8aea8da0f7f7039d2a2cecb14657d752a222a5e1d3825caeef186c82f751cdd1"

fetch_tool() {
    destination=$1
    url=$2
    expected_sha=$3
    if [ -f "$destination" ] && printf '%s  %s\n' "$expected_sha" "$destination" | sha256sum -c - >/dev/null 2>&1; then
        chmod +x "$destination"
        return
    fi
    temporary="$destination.download"
    curl --fail --location --output "$temporary" "$url"
    printf '%s  %s\n' "$expected_sha" "$temporary" | sha256sum -c -
    mv "$temporary" "$destination"
    chmod +x "$destination"
}

mkdir -p "$tool_dir" "$dist_dir"
fetch_tool "$linuxdeploy" "$linuxdeploy_url" "$linuxdeploy_sha"

cmake -E remove_directory "$appdir"
cmake -S "$repo_root/host" -B "$build_dir/host" \
    -DCMAKE_BUILD_TYPE=Release \
    -DAMB_BUILD_GUI=ON \
    -DCMAKE_INSTALL_PREFIX=/usr
cmake --build "$build_dir/host" --parallel "${CMAKE_BUILD_PARALLEL_LEVEL:-2}"
ctest --test-dir "$build_dir/host" --output-on-failure
DESTDIR="$appdir" cmake --install "$build_dir/host"

install -Dm0644 "$repo_root/linux/APPIMAGE_LICENSES.md" \
    "$appdir/usr/share/doc/mobile-webcam/APPIMAGE_LICENSES.md"
if [ -d /usr/share/licenses/qt6-base ]; then
    cp -a /usr/share/licenses/qt6-base \
        "$appdir/usr/share/doc/mobile-webcam/qt6-base-licenses"
fi
for license in GPL-3.0-only LGPL-2.1-or-later LGPL-3.0-only; do
    install -Dm0644 "$repo_root/linux/licenses/$license.txt" \
        "$appdir/usr/share/doc/mobile-webcam/licenses/$license.txt"
done

qt_plugins=$(/usr/bin/qmake6 -query QT_INSTALL_PLUGINS)
install -Dm0755 "$qt_plugins/platforms/libqxcb.so" \
    "$appdir/usr/plugins/platforms/libqxcb.so"
if [ -d "$qt_plugins/wayland-shell-integration" ]; then
    for plugin in "$qt_plugins/platforms/libqwayland"*.so; do
        [ -f "$plugin" ] || continue
        install -Dm0755 "$plugin" "$appdir/usr/plugins/platforms/$(basename "$plugin")"
    done
    for plugin_dir in wayland-decoration-client wayland-graphics-integration-client wayland-shell-integration; do
        for plugin in "$qt_plugins/$plugin_dir/"*.so; do
            [ -f "$plugin" ] || continue
            install -Dm0755 "$plugin" "$appdir/usr/plugins/$plugin_dir/$(basename "$plugin")"
        done
    done
fi
cat > "$appdir/usr/bin/qt.conf" <<'EOF'
[Paths]
Plugins = ../plugins
EOF

output="$dist_dir/Mobile_Webcam-$version-x86_64.AppImage"
stable_output="$dist_dir/Mobile_Webcam-x86_64.AppImage"
cmake -E rm -f "$output" "$output.sha256" "$stable_output"
set --
for plugin_dir in "$appdir/usr/plugins/"*; do
    [ -d "$plugin_dir" ] || continue
    set -- "$@" --deploy-deps-only "$plugin_dir"
done
PATH="$tool_dir:$PATH" \
APPIMAGE_EXTRACT_AND_RUN=1 \
LINUXDEPLOY_OUTPUT_VERSION="$version" \
LDAI_OUTPUT="$output" \
"$linuxdeploy" --appdir "$appdir" \
    "$@" \
    --library "$(pkg-config --variable=libdir libusb-1.0)/libusb-1.0.so.0" \
    --library "$(c++ -print-file-name=libstdc++.so.6)" \
    --library "$(cc -print-file-name=libgcc_s.so.1)" --output appimage

test -x "$output"
(cd "$dist_dir" && sha256sum "$(basename "$output")" > "$(basename "$output").sha256")
cmake -E create_symlink "$(basename "$output")" "$stable_output"
cat "$output.sha256"
