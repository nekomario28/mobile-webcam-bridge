#pragma once

#include <array>
#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string>
#include <vector>

namespace amb::wire {

constexpr std::uint32_t kMagic = 0x31424d41U;  // "AMB1" on the wire.
constexpr std::uint8_t kVersion = 1;
constexpr std::size_t kHeaderSize = 24;
constexpr std::uint32_t kMaxPayload = 16U * 1024U * 1024U;

constexpr std::uint16_t kVideoFlagKeyframe = 0x0001;
constexpr std::uint16_t kVideoFlagConfigIncluded = 0x0002;
constexpr std::uint16_t kVideoFlagDiscontinuity = 0x0004;

enum class Type : std::uint8_t {
    Hello = 0x01,
    HelloAck = 0x02,
    Ping = 0x03,
    Pong = 0x04,
    VideoConfig = 0x10,
    VideoAu = 0x11,
    VideoIdrRequest = 0x12,
    Stats = 0x20,
    Error = 0x7f,
};

struct Header {
    std::uint32_t magic{kMagic};
    std::uint8_t version{kVersion};
    Type type{Type::Error};
    std::uint16_t flags{};
    std::uint32_t sequence{};
    std::uint32_t payload_len{};
    std::uint64_t pts_us{};
};

std::array<std::uint8_t, kHeaderSize> encode_header(const Header& header);
std::optional<Header> decode_header(std::span<const std::uint8_t> bytes,
                                    std::string& error);
std::vector<std::uint8_t> make_frame(Type type, std::uint16_t flags,
                                     std::uint32_t sequence,
                                     std::uint64_t pts_us,
                                     std::span<const std::uint8_t> payload);

std::array<std::uint8_t, 8> encode_u64(std::uint64_t value);
std::optional<std::uint64_t> decode_u64(std::span<const std::uint8_t> bytes);

}  // namespace amb::wire
