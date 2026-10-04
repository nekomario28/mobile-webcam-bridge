#include <cstdint>
#include <cstring>

#include "../third_party/unity_capture/shared.inl"

#include <array>
#include <iostream>

namespace {
struct Received {
    std::array<std::uint8_t, 16> pixels{};
    bool valid = false;
};

void receive(int width, int height, int stride, SharedImageMemory::EFormat format,
             SharedImageMemory::EResizeMode, SharedImageMemory::EMirrorMode,
             int timeout, std::uint8_t* pixels, void* context) {
    auto& result = *static_cast<Received*>(context);
    result.valid = width == 2 && height == 2 && stride == 2 &&
                   format == SharedImageMemory::FORMAT_UINT8 && timeout == 1000;
    if (result.valid) std::memcpy(result.pixels.data(), pixels, result.pixels.size());
}
}  // namespace

int main() {
    // An unused device slot exercises the real Windows IPC without registering a camera.
    SharedImageMemory sender(40);
    if (sender.SendIsReady()) return 1;
    SharedImageMemory receiver(40);
    Received received;
    receiver.Receive(receive, &received);
    // Receiver and sender create their events in alternating steps.
    sender.SendIsReady();
    receiver.Receive(receive, &received);
    if (!sender.SendIsReady()) return 2;
    const std::array<std::uint8_t, 16> pixels{
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 1, 2, 3, 255};
    auto sent = sender.Send(2, 2, 2, pixels.size(), SharedImageMemory::FORMAT_UINT8,
                            SharedImageMemory::RESIZEMODE_DISABLED,
                            SharedImageMemory::MIRRORMODE_DISABLED, 1000, pixels.data());
    if (sent != SharedImageMemory::SENDRES_OK &&
        sent != SharedImageMemory::SENDRES_WARN_FRAMESKIP) return 3;
    if (receiver.Receive(receive, &received) != SharedImageMemory::RECEIVERES_NEWFRAME ||
        !received.valid || received.pixels != pixels) return 4;
    sent = sender.Send(2, 2, 2, MAX_SHARED_IMAGE_SIZE + 1,
                       SharedImageMemory::FORMAT_UINT8,
                       SharedImageMemory::RESIZEMODE_DISABLED,
                       SharedImageMemory::MIRRORMODE_DISABLED, 1000, pixels.data());
    if (sent != SharedImageMemory::SENDRES_TOOLARGE) return 5;
    std::cout << "unity_capture_test: PASS (IPC only)\n";
    return 0;
}
