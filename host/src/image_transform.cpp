#include "amb/image_transform.hpp"

#include <algorithm>
#include <charconv>
#include <limits>

namespace amb::video {
namespace {

bool valid_rgb24_buffer(std::span<const std::uint8_t> buffer, std::size_t width,
                        std::size_t height, std::size_t stride) {
    if (width == 0 || height == 0 ||
        width > std::numeric_limits<std::size_t>::max() / 3) {
        return false;
    }
    const std::size_t row_bytes = width * 3;
    return stride >= row_bytes && height <= buffer.size() / stride;
}

}  // namespace

std::optional<TransformSettings> parse_transform_settings(std::string_view line) {
    const auto first = line.find(' ');
    if (first == std::string_view::npos) return std::nullopt;
    const auto second = line.find(' ', first + 1);
    if (second == std::string_view::npos || line.find(' ', second + 1) != std::string_view::npos) {
        return std::nullopt;
    }

    int degrees = -1;
    const auto degrees_text = line.substr(0, first);
    const auto [end, error] = std::from_chars(
        degrees_text.data(), degrees_text.data() + degrees_text.size(), degrees);
    if (error != std::errc{} || end != degrees_text.data() + degrees_text.size()) {
        return std::nullopt;
    }

    Rotation rotation;
    switch (degrees) {
        case 0: rotation = Rotation::None; break;
        case 90: rotation = Rotation::Clockwise90; break;
        case 180: rotation = Rotation::Clockwise180; break;
        case 270: rotation = Rotation::Clockwise270; break;
        default: return std::nullopt;
    }
    const auto horizontal = line.substr(first + 1, second - first - 1);
    const auto vertical = line.substr(second + 1);
    if ((horizontal != "0" && horizontal != "1") ||
        (vertical != "0" && vertical != "1")) {
        return std::nullopt;
    }
    return TransformSettings{rotation, horizontal == "1", vertical == "1"};
}

std::size_t rotated_width(std::size_t width, std::size_t height, Rotation rotation) {
    return rotation == Rotation::Clockwise90 || rotation == Rotation::Clockwise270
               ? height
               : width;
}

std::size_t rotated_height(std::size_t width, std::size_t height, Rotation rotation) {
    return rotation == Rotation::Clockwise90 || rotation == Rotation::Clockwise270
               ? width
               : height;
}

std::optional<FitRect> fit_yuyv_rect(std::size_t content_width,
                                     std::size_t content_height,
                                     std::size_t output_width,
                                     std::size_t output_height) {
    if (content_width == 0 || content_height == 0 || output_width < 2 ||
        output_height == 0 || (output_width % 2) != 0 ||
        content_width > std::numeric_limits<std::size_t>::max() / output_height ||
        content_height > std::numeric_limits<std::size_t>::max() / output_width) {
        return std::nullopt;
    }
    std::size_t width = output_width;
    std::size_t height = output_height;
    if (content_width * output_height <= content_height * output_width) {
        width = (content_width * output_height / content_height) & ~std::size_t{1};
    } else {
        height = content_height * output_width / content_width;
    }
    if (width < 2 || height == 0) return std::nullopt;
    const std::size_t x = ((output_width - width) / 2) & ~std::size_t{1};
    return FitRect{x, (output_height - height) / 2, width, height};
}

bool rotate_rgb24(std::span<const std::uint8_t> source, std::size_t width,
                  std::size_t height, std::size_t source_stride,
                  std::span<std::uint8_t> destination,
                  std::size_t destination_stride, Rotation rotation) {
    const std::size_t output_width = rotated_width(width, height, rotation);
    const std::size_t output_height = rotated_height(width, height, rotation);
    if (!valid_rgb24_buffer(source, width, height, source_stride) ||
        !valid_rgb24_buffer(destination, output_width, output_height,
                            destination_stride)) {
        return false;
    }

    const auto copy_pixel = [&](std::size_t input_x, std::size_t input_y,
                                std::size_t output_x, std::size_t output_y) {
        const auto* input = source.data() + input_y * source_stride + input_x * 3;
        auto* output = destination.data() + output_y * destination_stride + output_x * 3;
        std::copy_n(input, 3, output);
    };
    switch (rotation) {
        case Rotation::None:
            for (std::size_t y = 0; y < height; ++y) {
                std::copy_n(source.data() + y * source_stride, width * 3,
                            destination.data() + y * destination_stride);
            }
            break;
        case Rotation::Clockwise90:
            for (std::size_t y = 0; y < height; ++y) {
                for (std::size_t x = 0; x < width; ++x) {
                    copy_pixel(x, y, height - y - 1, x);
                }
            }
            break;
        case Rotation::Clockwise180:
            for (std::size_t y = 0; y < height; ++y) {
                for (std::size_t x = 0; x < width; ++x) {
                    copy_pixel(x, y, width - x - 1, height - y - 1);
                }
            }
            break;
        case Rotation::Clockwise270:
            for (std::size_t y = 0; y < height; ++y) {
                for (std::size_t x = 0; x < width; ++x) {
                    copy_pixel(x, y, y, width - x - 1);
                }
            }
            break;
        default:
            return false;
    }
    return true;
}

}  // namespace amb::video
