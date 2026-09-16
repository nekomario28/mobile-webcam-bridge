#include "amb/aoa.hpp"

#include <libusb-1.0/libusb.h>

#include <array>
#include <chrono>
#include <iomanip>
#include <sstream>
#include <thread>

namespace amb::aoa {
namespace {
constexpr std::uint8_t kGetProtocol = 51;
constexpr std::uint8_t kSendString = 52;
constexpr std::uint8_t kStartAccessory = 53;
constexpr unsigned int kControlTimeoutMs = 1200;

constexpr std::uint8_t kInVendorDevice = static_cast<std::uint8_t>(
    static_cast<unsigned int>(LIBUSB_ENDPOINT_IN) |
    static_cast<unsigned int>(LIBUSB_REQUEST_TYPE_VENDOR) |
    static_cast<unsigned int>(LIBUSB_RECIPIENT_DEVICE));
constexpr std::uint8_t kOutVendorDevice = static_cast<std::uint8_t>(
    static_cast<unsigned int>(LIBUSB_ENDPOINT_OUT) |
    static_cast<unsigned int>(LIBUSB_REQUEST_TYPE_VENDOR) |
    static_cast<unsigned int>(LIBUSB_RECIPIENT_DEVICE));

DeviceId device_id(libusb_device* dev) {
    libusb_device_descriptor desc{};
    if (libusb_get_device_descriptor(dev, &desc) != 0) {
        return {};
    }
    return DeviceId{
        .bus = libusb_get_bus_number(dev),
        .address = libusb_get_device_address(dev),
        .vid = desc.idVendor,
        .pid = desc.idProduct,
    };
}

bool send_string(libusb_device_handle* handle, std::uint16_t index,
                 const std::string& value, std::string& error) {
    if (value.size() + 1 > 256) {
        error = "AOA identification string exceeds 256 bytes";
        return false;
    }
    std::vector<unsigned char> data(value.begin(), value.end());
    data.push_back('\0');
    const int rc = libusb_control_transfer(
        handle, kOutVendorDevice, kSendString, 0, index, data.data(),
        static_cast<std::uint16_t>(data.size()), kControlTimeoutMs);
    if (rc < 0) {
        error = "AOA SEND_STRING index " + std::to_string(index) + ": " +
                libusb_error_name(rc);
        return false;
    }
    if (rc != static_cast<int>(data.size())) {
        error = "AOA SEND_STRING index " + std::to_string(index) +
                " transferred a short payload";
        return false;
    }
    return true;
}
}  // namespace

bool is_accessory_pid(std::uint16_t pid) {
    return pid == kAccessoryPid || pid == kAccessoryAdbPid ||
           pid == kAccessoryAudioPid || pid == kAccessoryAudioAdbPid;
}

std::vector<DeviceId> list_devices(libusb_context* ctx) {
    std::vector<DeviceId> out;
    libusb_device** list = nullptr;
    const auto count = libusb_get_device_list(ctx, &list);
    if (count < 0) return out;
    for (ssize_t i = 0; i < count; ++i) {
        const auto id = device_id(list[i]);
        if (id.vid != 0) out.push_back(id);
    }
    libusb_free_device_list(list, 1);
    return out;
}

ProbeResult probe_protocol(libusb_device* dev) {
    ProbeResult result{};
    result.id = device_id(dev);

    libusb_device_handle* handle = nullptr;
    int rc = libusb_open(dev, &handle);
    if (rc < 0) {
        result.error = libusb_error_name(rc);
        return result;
    }

    std::array<unsigned char, 2> bytes{};
    rc = libusb_control_transfer(handle, kInVendorDevice, kGetProtocol,
                                 0, 0, bytes.data(), bytes.size(),
                                 kControlTimeoutMs);
    libusb_close(handle);

    if (rc == 2) {
        result.protocol = static_cast<std::uint16_t>(bytes[0]) |
                          (static_cast<std::uint16_t>(bytes[1]) << 8);
    } else if (rc < 0) {
        result.error = libusb_error_name(rc);
    } else {
        result.error = "short GET_PROTOCOL response";
    }
    return result;
}

bool request_accessory_mode(libusb_device* dev, std::string& error) {
    libusb_device_handle* handle = nullptr;
    int rc = libusb_open(dev, &handle);
    if (rc < 0) {
        error = std::string("libusb_open: ") + libusb_error_name(rc);
        return false;
    }

    std::array<unsigned char, 2> bytes{};
    rc = libusb_control_transfer(handle, kInVendorDevice, kGetProtocol,
                                 0, 0, bytes.data(), bytes.size(),
                                 kControlTimeoutMs);
    if (rc != 2) {
        error = rc < 0 ? std::string("AOA GET_PROTOCOL: ") + libusb_error_name(rc)
                       : "AOA GET_PROTOCOL returned a short response";
        libusb_close(handle);
        return false;
    }

    const auto protocol = static_cast<std::uint16_t>(bytes[0]) |
                          (static_cast<std::uint16_t>(bytes[1]) << 8);
    if (protocol == 0) {
        error = "device reported AOA protocol 0";
        libusb_close(handle);
        return false;
    }

    // IDs 0..5 are defined by the AOA 1.0 protocol.
    const std::array<std::string, 6> strings = {
        "Mobile Webcam",
        "Mobile Webcam Host",
        "Android camera to Linux webcam bridge",
        "0",
        "https://github.com/nekomario28",
        "mobile-webcam",
    };
    for (std::uint16_t i = 0; i < strings.size(); ++i) {
        if (!send_string(handle, i, strings[i], error)) {
            libusb_close(handle);
            return false;
        }
    }

    rc = libusb_control_transfer(handle, kOutVendorDevice, kStartAccessory,
                                 0, 0, nullptr, 0, kControlTimeoutMs);
    libusb_close(handle);
    if (rc < 0) {
        error = std::string("AOA START_ACCESSORY: ") + libusb_error_name(rc);
        return false;
    }
    return true;
}

std::optional<DeviceId> wait_for_accessory(libusb_context* ctx, int timeout_ms,
                                           std::optional<std::uint8_t> expected_bus) {
    const auto deadline = std::chrono::steady_clock::now() +
                          std::chrono::milliseconds(timeout_ms);
    while (std::chrono::steady_clock::now() < deadline) {
        libusb_device** list = nullptr;
        const auto count = libusb_get_device_list(ctx, &list);
        if (count >= 0) {
            for (ssize_t i = 0; i < count; ++i) {
                const auto id = device_id(list[i]);
                if (id.vid == kGoogleVid && is_accessory_pid(id.pid) &&
                    (!expected_bus || id.bus == *expected_bus)) {
                    libusb_free_device_list(list, 1);
                    return id;
                }
            }
            libusb_free_device_list(list, 1);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    return std::nullopt;
}

std::string describe(const DeviceId& id) {
    std::ostringstream out;
    out << "bus=" << static_cast<int>(id.bus)
        << " addr=" << static_cast<int>(id.address)
        << " VID=" << std::hex << std::setw(4) << std::setfill('0') << id.vid
        << " PID=" << std::setw(4) << id.pid << std::dec;
    return out.str();
}

}  // namespace amb::aoa
