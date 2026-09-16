#include "amb/accessory.hpp"
#include "amb/wire.hpp"

#include <libusb-1.0/libusb.h>

#include <charconv>
#include <chrono>
#include <cstdint>
#include <fstream>
#include <iostream>
#include <string>
#include <vector>

namespace {
std::uint16_t get_u16(const std::uint8_t* p) {
    return static_cast<std::uint16_t>(p[0]) |
           (static_cast<std::uint16_t>(p[1]) << 8);
}
std::uint32_t get_u32(const std::uint8_t* p) {
    std::uint32_t v = 0;
    for (int i = 0; i < 4; ++i) v |= static_cast<std::uint32_t>(p[i]) << (8 * i);
    return v;
}

struct Options {
    std::string output = "capture.h264";
    std::size_t frames = 300;
    int timeout_ms = 30'000;
};

void usage(const char* argv0) {
    std::cout
        << "Usage: " << argv0 << " [--output FILE] [--frames N] [--timeout-ms N]\n"
        << "\n"
        << "Connects to an already-open AOA accessory session and writes VIDEO_AU\n"
        << "payloads to an Annex-B .h264 file. Start the G1 camera from the phone UI.\n";
}

bool parse_positive(const std::string& text, std::size_t& out) {
    unsigned long long value = 0;
    const auto [ptr, ec] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (ec != std::errc{} || ptr != text.data() + text.size() || value == 0) return false;
    out = static_cast<std::size_t>(value);
    return true;
}
}  // namespace

int main(int argc, char** argv) {
    Options options{};
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--output" && i + 1 < argc) {
            options.output = argv[++i];
        } else if (arg == "--frames" && i + 1 < argc) {
            if (!parse_positive(argv[++i], options.frames)) {
                std::cerr << "invalid --frames\n";
                return 2;
            }
        } else if (arg == "--timeout-ms" && i + 1 < argc) {
            std::size_t parsed = 0;
            if (!parse_positive(argv[++i], parsed) || parsed > 600'000) {
                std::cerr << "invalid --timeout-ms\n";
                return 2;
            }
            options.timeout_ms = static_cast<int>(parsed);
        } else if (arg == "-h" || arg == "--help") {
            usage(argv[0]);
            return 0;
        } else {
            std::cerr << "unknown or incomplete argument: " << arg << "\n";
            usage(argv[0]);
            return 2;
        }
    }

    libusb_context* ctx = nullptr;
    const int init_rc = libusb_init(&ctx);
    if (init_rc < 0) {
        std::cerr << "libusb_init failed: " << libusb_error_name(init_rc) << "\n";
        return 1;
    }

    int result = 0;
    {
        amb::AccessoryDevice accessory(ctx);
        std::string error;
        if (!accessory.connect(std::nullopt, error)) {
            std::cerr << "accessory connect failed: " << error << "\n";
            result = 3;
        } else {
            std::cout << "connected " << amb::aoa::describe(accessory.id()) << "\n";
            std::ofstream out(options.output, std::ios::binary | std::ios::trunc);
            if (!out) {
                std::cerr << "unable to open output file: " << options.output << "\n";
                result = 4;
            } else {
                if (!accessory.send_frame(amb::wire::Type::VideoIdrRequest, 0, 1, 0,
                                          {}, 2000, error)) {
                    std::cerr << "warning: initial IDR request failed: " << error << "\n";
                    error.clear();
                }

                std::size_t video_frames = 0;
                std::size_t keyframes = 0;
                std::size_t discontinuities = 0;
                std::uint64_t first_pts_us = 0;
                std::uint64_t last_pts_us = 0;
                std::chrono::steady_clock::time_point first_wall;
                while (video_frames < options.frames) {
                    amb::wire::Header header{};
                    std::vector<std::uint8_t> payload;
                    if (!accessory.read_frame(header, payload, options.timeout_ms, error)) {
                        std::cerr << "read failed after " << video_frames
                                  << " video frames: " << error << "\n";
                        result = 5;
                        break;
                    }

                    if (header.type == amb::wire::Type::VideoConfig) {
                        if (payload.size() != 16) {
                            std::cerr << "invalid VIDEO_CONFIG length=" << payload.size() << "\n";
                            result = 6;
                            break;
                        }
                        const auto width = get_u16(payload.data() + 0);
                        const auto height = get_u16(payload.data() + 2);
                        const auto fps = get_u16(payload.data() + 4);
                        const auto bitrate = get_u32(payload.data() + 8);
                        const auto codec = payload[12];
                        const auto nal_format = payload[13];
                        std::cout << "VIDEO_CONFIG " << width << "x" << height
                                  << " @" << fps << " bitrate=" << bitrate
                                  << " codec=" << static_cast<unsigned>(codec)
                                  << " nal=" << static_cast<unsigned>(nal_format) << "\n";
                        if (codec != 1 || nal_format != 1) {
                            std::cerr << "unsupported video config; expected H.264 Annex-B\n";
                            result = 7;
                            break;
                        }
                        continue;
                    }

                    if (header.type != amb::wire::Type::VideoAu) {
                        std::cout << "ignoring message type=0x" << std::hex
                                  << static_cast<unsigned>(header.type) << std::dec << "\n";
                        continue;
                    }
                    if (payload.empty()) {
                        std::cerr << "empty VIDEO_AU\n";
                        result = 8;
                        break;
                    }
                    // A running camera may have queued inter-frames before the
                    // host's IDR request. Start a self-contained Annex-B stream.
                    if (video_frames == 0 &&
                        ((header.flags & amb::wire::kVideoFlagKeyframe) == 0 ||
                         (header.flags & amb::wire::kVideoFlagConfigIncluded) == 0)) {
                        continue;
                    }
                    if (video_frames > 0 && header.pts_us <= last_pts_us) {
                        std::cerr << "non-monotonic VIDEO_AU PTS\n";
                        result = 10;
                        break;
                    }

                    out.write(reinterpret_cast<const char*>(payload.data()),
                              static_cast<std::streamsize>(payload.size()));
                    if (!out) {
                        std::cerr << "write failed for " << options.output << "\n";
                        result = 9;
                        break;
                    }
                    if (video_frames == 0) {
                        first_pts_us = header.pts_us;
                        first_wall = std::chrono::steady_clock::now();
                    }
                    last_pts_us = header.pts_us;
                    ++video_frames;
                    if (header.flags & amb::wire::kVideoFlagKeyframe) ++keyframes;
                    if (video_frames > 1 &&
                        (header.flags & amb::wire::kVideoFlagDiscontinuity)) ++discontinuities;

                    if (video_frames == 1 || video_frames % 30 == 0) {
                        std::cout << "frames=" << video_frames
                                  << " keyframes=" << keyframes
                                  << " discontinuities=" << discontinuities
                                  << " pts_us=" << header.pts_us << "\n";
                    }
                }

                out.flush();
                if (result == 0) {
                    std::cout << "[G1 transport slice] wrote " << video_frames
                              << " Annex-B access units to " << options.output
                              << " keyframes=" << keyframes
                              << " discontinuities=" << discontinuities
                              << " pts_span_s=" << (last_pts_us - first_pts_us) / 1'000'000.0
                              << " wall_s=" << std::chrono::duration<double>(
                                  std::chrono::steady_clock::now() - first_wall).count()
                              << "\n";
                }
            }
        }
    }
    libusb_exit(ctx);
    return result;
}
