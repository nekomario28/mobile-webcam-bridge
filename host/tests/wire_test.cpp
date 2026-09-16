#include "amb/wire.hpp"

#include <array>
#include <cstdint>
#include <iostream>
#include <string>

namespace {
bool expect(bool condition, const char* message) {
    if (condition) return true;
    std::cerr << "wire_test: FAIL: " << message << '\n';
    return false;
}
}  // namespace

int main() {
    using namespace amb::wire;
    bool ok = true;

    const std::array<std::uint8_t, 8> payload{0, 1, 2, 3, 4, 5, 6, 7};
    const auto frame = make_frame(Type::Ping, 0x1234, 0x11223344, 0x0102030405060708ULL,
                                  payload);
    ok &= expect(frame.size() == kHeaderSize + payload.size(), "frame size");
    ok &= expect(frame[0] == 'A' && frame[1] == 'M' && frame[2] == 'B' && frame[3] == '1',
                 "magic bytes");

    std::string error;
    const auto header = decode_header(std::span<const std::uint8_t>(frame.data(), kHeaderSize), error);
    ok &= expect(header.has_value(), "decode valid header");
    if (header) {
        ok &= expect(header->type == Type::Ping, "header type");
        ok &= expect(header->flags == 0x1234, "header flags");
        ok &= expect(header->sequence == 0x11223344, "header sequence");
        ok &= expect(header->payload_len == payload.size(), "header payload length");
        ok &= expect(header->pts_us == 0x0102030405060708ULL, "header PTS");
    }

    auto broken = frame;
    broken[0] = 'X';
    error.clear();
    ok &= expect(!decode_header(std::span<const std::uint8_t>(broken.data(), kHeaderSize), error),
                 "reject broken magic");
    ok &= expect(!error.empty(), "broken magic reports an error");

    const auto encoded = encode_u64(0x8877665544332211ULL);
    const auto decoded = decode_u64(encoded);
    ok &= expect(decoded && *decoded == 0x8877665544332211ULL, "u64 round trip");

    if (!ok) return 1;
    std::cout << "wire_test: PASS\n";
    return 0;
}
