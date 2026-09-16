#pragma once

#include <cstddef>
#include <cstdint>
#include <span>

namespace amb::video {

bool flip_yuyv422_horizontal(std::span<std::uint8_t> frame, std::size_t width,
                             std::size_t height, std::size_t stride);
bool flip_yuyv422_vertical(std::span<std::uint8_t> frame, std::size_t width,
                           std::size_t height, std::size_t stride);

}  // namespace amb::video
