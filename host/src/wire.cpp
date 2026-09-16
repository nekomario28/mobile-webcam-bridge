#include "amb/wire.hpp"

#include <limits>

namespace amb::wire {
namespace {
void put_u16(std::uint8_t* p, std::uint16_t v) {
    p[0] = static_cast<std::uint8_t>(v);
    p[1] = static_cast<std::uint8_t>(v >> 8);
}
void put_u32(std::uint8_t* p, std::uint32_t v) {
    for (int i = 0; i < 4; ++i) p[i] = static_cast<std::uint8_t>(v >> (8 * i));
}
void put_u64(std::uint8_t* p, std::uint64_t v) {
    for (int i = 0; i < 8; ++i) p[i] = static_cast<std::uint8_t>(v >> (8 * i));
}
std::uint16_t get_u16(const std::uint8_t* p) {
    return static_cast<std::uint16_t>(p[0]) |
           (static_cast<std::uint16_t>(p[1]) << 8);
}
std::uint32_t get_u32(const std::uint8_t* p) {
    std::uint32_t v = 0;
    for (int i = 0; i < 4; ++i) v |= static_cast<std::uint32_t>(p[i]) << (8 * i);
    return v;
}
std::uint64_t get_u64(const std::uint8_t* p) {
    std::uint64_t v = 0;
    for (int i = 0; i < 8; ++i) v |= static_cast<std::uint64_t>(p[i]) << (8 * i);
    return v;
}
}  // namespace

std::array<std::uint8_t, kHeaderSize> encode_header(const Header& header) {
    std::array<std::uint8_t, kHeaderSize> out{};
    put_u32(out.data() + 0, header.magic);
    out[4] = header.version;
    out[5] = static_cast<std::uint8_t>(header.type);
    put_u16(out.data() + 6, header.flags);
    put_u32(out.data() + 8, header.sequence);
    put_u32(out.data() + 12, header.payload_len);
    put_u64(out.data() + 16, header.pts_us);
    return out;
}

std::optional<Header> decode_header(std::span<const std::uint8_t> bytes,
                                    std::string& error) {
    if (bytes.size() != kHeaderSize) {
        error = "wire header is not 24 bytes";
        return std::nullopt;
    }
    Header h{};
    h.magic = get_u32(bytes.data() + 0);
    h.version = bytes[4];
    h.type = static_cast<Type>(bytes[5]);
    h.flags = get_u16(bytes.data() + 6);
    h.sequence = get_u32(bytes.data() + 8);
    h.payload_len = get_u32(bytes.data() + 12);
    h.pts_us = get_u64(bytes.data() + 16);

    if (h.magic != kMagic) {
        error = "wire magic mismatch";
        return std::nullopt;
    }
    if (h.version != kVersion) {
        error = "unsupported wire version " + std::to_string(h.version);
        return std::nullopt;
    }
    if (h.payload_len > kMaxPayload) {
        error = "payload exceeds hard limit";
        return std::nullopt;
    }
    return h;
}

std::vector<std::uint8_t> make_frame(Type type, std::uint16_t flags,
                                     std::uint32_t sequence,
                                     std::uint64_t pts_us,
                                     std::span<const std::uint8_t> payload) {
    if (payload.size() > kMaxPayload ||
        payload.size() > std::numeric_limits<std::uint32_t>::max()) {
        return {};
    }
    const Header h{
        .magic = kMagic,
        .version = kVersion,
        .type = type,
        .flags = flags,
        .sequence = sequence,
        .payload_len = static_cast<std::uint32_t>(payload.size()),
        .pts_us = pts_us,
    };
    const auto encoded = encode_header(h);
    std::vector<std::uint8_t> out;
    out.reserve(encoded.size() + payload.size());
    out.insert(out.end(), encoded.begin(), encoded.end());
    out.insert(out.end(), payload.begin(), payload.end());
    return out;
}

std::array<std::uint8_t, 8> encode_u64(std::uint64_t value) {
    std::array<std::uint8_t, 8> out{};
    put_u64(out.data(), value);
    return out;
}

std::optional<std::uint64_t> decode_u64(std::span<const std::uint8_t> bytes) {
    if (bytes.size() != 8) return std::nullopt;
    return get_u64(bytes.data());
}

}  // namespace amb::wire
