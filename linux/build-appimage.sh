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
linuxdeploy_sha="36a2d7e274d12e1050d0e9ecfe11d339ed54720b2bec464c286d53f8b07f5c62"

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
cmake --build "$build_dir/host" --parallel
ctest --test-dir "$build_dir/host" --output-on-failure
DESTDIR="$appdir" cmake --install "$build_dir/host"

install -Dm0644 "$repo_root/linux/APPIMAGE_LICENSES.md" \
    "$appdir/usr/share/doc/mobile-webcam/APPIMAGE_LICENSES.md"
if [ -d /usr/share/licenses/qt6-base ]; then
    cp -a /usr/share/licenses/qt6-base \
        "$appdir/usr/share/doc/mobile-webcam/qt6-base-licenses"
fi

qt_plugins=$(/usr/bin/qmake6 -query QT_INSTALL_PLUGINS)
install -Dm0755 "$qt_plugins/platforms/libqxcb.so" \
    "$appdir/usr/plugins/platforms/libqxcb.so"
if [ -f "$qt_plugins/platforms/libqwayland.so" ]; then
    install -Dm0755 "$qt_plugins/platforms/libqwayland.so" \
        "$appdir/usr/plugins/platforms/libqwayland.so"
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
PATH="$tool_dir:$PATH" \
APPIMAGE_EXTRACT_AND_RUN=1 \
LINUXDEPLOY_OUTPUT_VERSION="$version" \
LDAI_OUTPUT="$output" \
"$linuxdeploy" --appdir "$appdir" --output appimage

test -x "$output"
(cd "$dist_dir" && sha256sum "$(basename "$output")" > "$(basename "$output").sha256")
cmake -E create_symlink "$(basename "$output")" "$stable_output"
cat "$output.sha256"
