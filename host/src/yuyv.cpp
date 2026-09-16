#include "amb/yuyv.hpp"

#include <algorithm>
#include <array>
#include <limits>

namespace amb::video {

bool flip_yuyv422_horizontal(std::span<std::uint8_t> frame, std::size_t width,
                             std::size_t height, std::size_t stride) {
    if (width == 0 || height == 0 || (width % 2) != 0 ||
        width > std::numeric_limits<std::size_t>::max() / 2) {
        return false;
    }
    const std::size_t row_bytes = width * 2;
    if (stride < row_bytes || height > frame.size() / stride) return false;

    const std::size_t pairs = width / 2;
    for (std::size_t y = 0; y < height; ++y) {
        auto* row = frame.data() + y * stride;
        for (std::size_t pair = 0; pair < pairs; ++pair) {
            std::swap(row[pair * 4], row[pair * 4 + 2]);
        }
        for (std::size_t left = 0; left < pairs / 2; ++left) {
            const std::size_t right = pairs - left - 1;
            for (std::size_t byte = 0; byte < 4; ++byte) {
                std::swap(row[left * 4 + byte], row[right * 4 + byte]);
            }
        }
    }
    return true;
}

bool flip_yuyv422_vertical(std::span<std::uint8_t> frame, std::size_t width,
                           std::size_t height, std::size_t stride) {
    if (width == 0 || height == 0 || (width % 2) != 0 ||
        width > std::numeric_limits<std::size_t>::max() / 2) {
        return false;
    }
    const std::size_t row_bytes = width * 2;
    if (stride < row_bytes || height > frame.size() / stride) return false;

    for (std::size_t top = 0; top < height / 2; ++top) {
        auto* top_row = frame.data() + top * stride;
        auto* bottom_row = frame.data() + (height - top - 1) * stride;
        std::swap_ranges(top_row, top_row + row_bytes, bottom_row);
    }
    return true;
}

}  // namespace amb::video
