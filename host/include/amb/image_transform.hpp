#pragma once

#include <cstddef>
#include <cstdint>
#include <optional>
#include <span>
#include <string_view>

namespace amb::video {

enum class Rotation : int {
    None = 0,
    Clockwise90 = 90,
    Clockwise180 = 180,
    Clockwise270 = 270,
};

struct TransformSettings {
    Rotation rotation = Rotation::None;
    bool horizontal_flip = true;
    bool vertical_flip = false;
};

std::optional<TransformSettings> parse_transform_settings(std::string_view line);

std::size_t rotated_width(std::size_t width, std::size_t height, Rotation rotation);
std::size_t rotated_height(std::size_t width, std::size_t height, Rotation rotation);

struct FitRect {
    std::size_t x;
    std::size_t y;
    std::size_t width;
    std::size_t height;
};

std::optional<FitRect> fit_yuyv_rect(std::size_t content_width,
                                     std::size_t content_height,
                                     std::size_t output_width,
                                     std::size_t output_height);

bool rotate_rgb24(std::span<const std::uint8_t> source, std::size_t width,
                  std::size_t height, std::size_t source_stride,
                  std::span<std::uint8_t> destination,
                  std::size_t destination_stride, Rotation rotation);

}  // namespace amb::video
