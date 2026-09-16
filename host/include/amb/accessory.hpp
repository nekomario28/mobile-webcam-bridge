#pragma once

#include "amb/aoa.hpp"
#include "amb/wire.hpp"

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <vector>

struct libusb_context;
struct libusb_device_handle;

namespace amb {

class AccessoryDevice {
  public:
    explicit AccessoryDevice(libusb_context* ctx);
    ~AccessoryDevice();

    AccessoryDevice(const AccessoryDevice&) = delete;
    AccessoryDevice& operator=(const AccessoryDevice&) = delete;

    bool connect(std::optional<aoa::DeviceId> target, std::string& error);
    void disconnect();
    [[nodiscard]] bool connected() const { return handle_ != nullptr; }
    [[nodiscard]] aoa::DeviceId id() const { return id_; }

    bool send_frame(wire::Type type, std::uint16_t flags, std::uint32_t sequence,
                    std::uint64_t pts_us, std::span<const std::uint8_t> payload,
                    int timeout_ms, std::string& error);
    bool read_frame(wire::Header& header, std::vector<std::uint8_t>& payload,
                    int timeout_ms, std::string& error);

    bool smoke_test(std::size_t iterations, int timeout_ms, std::string& error);

  private:
    bool write_all(const std::uint8_t* data, std::size_t size, int timeout_ms,
                   std::string& error);
    bool read_exact(std::uint8_t* data, std::size_t size, int timeout_ms,
                    std::string& error);

    libusb_context* ctx_{};
    libusb_device_handle* handle_{};
    int interface_ = -1;
    std::uint8_t in_endpoint_{};
    std::uint8_t out_endpoint_{};
    std::uint16_t in_packet_size_{};
    aoa::DeviceId id_{};
};

}  // namespace amb
