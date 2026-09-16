#include "amb/image_transform.hpp"

#include <array>
#include <cstdint>
#include <iostream>

namespace {
bool expect(bool condition, const char* message) {
    if (condition) return true;
    std::cerr << "image_transform_test: FAIL: " << message << '\n';
    return false;
}
}  // namespace

int main() {
    using amb::video::Rotation;
    bool ok = true;

    const auto transform = amb::video::parse_transform_settings("270 0 1");
    ok &= expect(transform && transform->rotation == Rotation::Clockwise270 &&
                     !transform->horizontal_flip && transform->vertical_flip,
                 "parse complete live transform command");
    ok &= expect(!amb::video::parse_transform_settings("90 1"),
                 "reject obsolete two-field transform command");
    ok &= expect(!amb::video::parse_transform_settings("45 1 0"),
                 "reject unsupported rotation");
    ok &= expect(!amb::video::parse_transform_settings("90 2 0"),
                 "reject invalid flip values");
    const std::array<std::uint8_t, 18> source{
        1, 0, 0, 2, 0, 0, 3, 0, 0,
        4, 0, 0, 5, 0, 0, 6, 0, 0,
    };
    std::array<std::uint8_t, 18> output{};

    ok &= expect(amb::video::rotated_width(3, 2, Rotation::Clockwise90) == 2 &&
                     amb::video::rotated_height(3, 2, Rotation::Clockwise90) == 3,
                 "swap dimensions for quarter turn");

    const auto portrait = amb::video::fit_yuyv_rect(720, 1280, 1280, 720);
    ok &= expect(portrait && portrait->x == 438 && portrait->y == 0 &&
                     portrait->width == 404 && portrait->height == 720,
                 "portrait fits centered inside unchanged 1280x720 YUYV output");
    const auto landscape = amb::video::fit_yuyv_rect(1280, 720, 1280, 720);
    ok &= expect(landscape && landscape->x == 0 && landscape->y == 0 &&
                     landscape->width == 1280 && landscape->height == 720,
                 "landscape retains full output size");
    ok &= expect(!amb::video::fit_yuyv_rect(720, 1280, 1279, 720),
                 "reject odd YUYV output width");

    const std::array<std::uint8_t, 18> expected90{
        4, 0, 0, 1, 0, 0,
        5, 0, 0, 2, 0, 0,
        6, 0, 0, 3, 0, 0,
    };
    ok &= expect(amb::video::rotate_rgb24(source, 3, 2, 9, output, 6,
                                          Rotation::Clockwise90),
                 "rotate 90 degrees");
    ok &= expect(output == expected90, "90 degree pixel positions");

    const std::array<std::uint8_t, 18> expected180{
        6, 0, 0, 5, 0, 0, 4, 0, 0,
        3, 0, 0, 2, 0, 0, 1, 0, 0,
    };
    ok &= expect(amb::video::rotate_rgb24(source, 3, 2, 9, output, 9,
                                          Rotation::Clockwise180),
                 "rotate 180 degrees");
    ok &= expect(output == expected180, "180 degree pixel positions");

    const std::array<std::uint8_t, 18> expected270{
        3, 0, 0, 6, 0, 0,
        2, 0, 0, 5, 0, 0,
        1, 0, 0, 4, 0, 0,
    };
    ok &= expect(amb::video::rotate_rgb24(source, 3, 2, 9, output, 6,
                                          Rotation::Clockwise270),
                 "rotate 270 degrees");
    ok &= expect(output == expected270, "270 degree pixel positions");

    ok &= expect(!amb::video::rotate_rgb24(source, 3, 2, 9, output, 5,
                                           Rotation::Clockwise90),
                 "reject short destination stride");
    ok &= expect(!amb::video::rotate_rgb24(source, 3, 2, 9,
                                           std::span<std::uint8_t>(output).first(12), 6,
                                           Rotation::Clockwise90),
                 "reject short destination buffer");

    if (!ok) return 1;
    std::cout << "image_transform_test: PASS\n";
    return 0;
}
