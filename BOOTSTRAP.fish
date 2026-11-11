# Run from the repository root on CachyOS / Arch.
# The classic C++ probe tools (host/) were removed; this uses the Rust host.
sudo pacman -S --needed base-devel pkgconf libusb ffmpeg clang v4l2loopback-dkms v4l2loopback-utils
cargo build --manifest-path host-rs/Cargo.toml --release --locked

# Install access for accessory-only USB mode and the virtual-camera name.
sudo ./linux/install-host-integration.sh

# Safe discovery: no AOA vendor request is sent.
./host-rs/target/release/mobile-webcam-usb --list

# After the Android app is open (USB mode), run the G0/G0.5 gate end to end:
# AOA switch, per-PC authorization, then 1000 PING/PONG round trips.
# ./host-rs/target/release/mobile-webcam --usb-probe --frames 1000
