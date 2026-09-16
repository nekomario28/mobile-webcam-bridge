#include "amb/yuyv.hpp"

#include <array>
#include <cstdint>
#include <iostream>

namespace {
bool expect(bool condition, const char* message) {
    if (condition) return true;
    std::cerr << "yuyv_test: FAIL: " << message << '\n';
    return false;
}
}  // namespace

int main() {
    bool ok = true;
    std::array<std::uint8_t, 16> frame{
        1, 10, 2, 20,
        3, 30, 4, 40,
        5, 50, 6, 60,
        0xee, 0xee, 0xee, 0xee,
    };
    const auto original = frame;
    const std::array<std::uint8_t, 16> expected{
        6, 50, 5, 60,
        4, 30, 3, 40,
        2, 10, 1, 20,
        0xee, 0xee, 0xee, 0xee,
    };

    ok &= expect(amb::video::flip_yuyv422_horizontal(frame, 6, 1, 16),
                 "accept valid frame");
    ok &= expect(frame == expected, "mirror pixels and preserve chroma pairs");
    ok &= expect(amb::video::flip_yuyv422_horizontal(frame, 6, 1, 16),
                 "flip twice");
    ok &= expect(frame == original, "double flip restores input");
    ok &= expect(!amb::video::flip_yuyv422_horizontal(frame, 5, 1, 16),
                 "reject odd width");
    ok &= expect(!amb::video::flip_yuyv422_horizontal(frame, 6, 2, 16),
                 "reject short buffer");

    std::array<std::uint8_t, 24> vertical{
        1, 10, 2, 20, 3, 30, 4, 40, 0xaa, 0xaa, 0xaa, 0xaa,
        5, 50, 6, 60, 7, 70, 8, 80, 0xbb, 0xbb, 0xbb, 0xbb,
    };
    const auto vertical_original = vertical;
    const std::array<std::uint8_t, 24> vertical_expected{
        5, 50, 6, 60, 7, 70, 8, 80, 0xaa, 0xaa, 0xaa, 0xaa,
        1, 10, 2, 20, 3, 30, 4, 40, 0xbb, 0xbb, 0xbb, 0xbb,
    };
    ok &= expect(amb::video::flip_yuyv422_vertical(vertical, 4, 2, 12),
                 "accept valid vertical flip");
    ok &= expect(vertical == vertical_expected,
                 "swap active rows and preserve stride padding");
    ok &= expect(amb::video::flip_yuyv422_vertical(vertical, 4, 2, 12),
                 "vertical flip twice");
    ok &= expect(vertical == vertical_original,
                 "double vertical flip restores input");
    ok &= expect(!amb::video::flip_yuyv422_vertical(vertical, 3, 2, 12),
                 "reject odd width for vertical flip");
    ok &= expect(!amb::video::flip_yuyv422_vertical(vertical, 4, 3, 12),
                 "reject short vertical buffer");

    if (!ok) return 1;
    std::cout << "yuyv_test: PASS\n";
    return 0;
}
