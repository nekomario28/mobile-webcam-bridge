# Run from the repository root on CachyOS / Arch.
sudo pacman -S --needed base-devel cmake pkgconf libusb ffmpeg v4l2loopback-dkms v4l2loopback-utils
cmake -S host -B build/host -DCMAKE_BUILD_TYPE=Release
cmake --build build/host --parallel
ctest --test-dir build/host --output-on-failure

# Install access for accessory-only USB mode and the virtual-camera name.
sudo ./linux/install-host-integration.sh

# Safe discovery: no AOA vendor request is sent.
sudo ./build/host/amb-aoa-probe --list

# Then use the Xperia VID:PID printed above, for example:
# sudo ./build/host/amb-aoa-probe --device 0fce:XXXX
# sudo ./build/host/amb-aoa-probe --device 0fce:XXXX --switch
# After the Android app opens the accessory:
# ./build/host/amb-aoa-probe --smoke 1000
