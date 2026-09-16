#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

struct libusb_context;
struct libusb_device;

namespace amb::aoa {

constexpr std::uint16_t kGoogleVid = 0x18d1;
constexpr std::uint16_t kAccessoryPid = 0x2d00;
constexpr std::uint16_t kAccessoryAdbPid = 0x2d01;
constexpr std::uint16_t kAccessoryAudioPid = 0x2d04;
constexpr std::uint16_t kAccessoryAudioAdbPid = 0x2d05;

struct DeviceId {
    std::uint8_t bus{};
    std::uint8_t address{};
    std::uint16_t vid{};
    std::uint16_t pid{};
};

struct ProbeResult {
    DeviceId id{};
    std::optional<std::uint16_t> protocol;
    std::string error;
};

bool is_accessory_pid(std::uint16_t pid);
std::vector<DeviceId> list_devices(libusb_context* ctx);
ProbeResult probe_protocol(libusb_device* dev);
bool request_accessory_mode(libusb_device* dev, std::string& error);
std::optional<DeviceId> wait_for_accessory(libusb_context* ctx, int timeout_ms,
                                           std::optional<std::uint8_t> expected_bus = std::nullopt);
std::string describe(const DeviceId& id);

}  // namespace amb::aoa
