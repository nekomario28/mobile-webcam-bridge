#include "amb/accessory.hpp"
#include "amb/hw_decode.hpp"
#include "amb/image_transform.hpp"
#include "amb/tcp.hpp"
#include "amb/wire.hpp"
#include "amb/yuyv.hpp"

#include <libusb-1.0/libusb.h>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavutil/error.h>
#include <libavutil/frame.h>
#include <libavutil/pixfmt.h>
#include <libswscale/swscale.h>
}

#include <linux/videodev2.h>
#include <sys/ioctl.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <charconv>
#include <chrono>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <iostream>
#include <limits>
#include <memory>
#include <span>
#include <string>
#include <vector>

namespace {

std::uint16_t get_u16(const std::uint8_t* p) {
    return static_cast<std::uint16_t>(p[0]) |
           (static_cast<std::uint16_t>(p[1]) << 8);
}

std::string av_error(int code) {
    char text[AV_ERROR_MAX_STRING_SIZE]{};
    av_strerror(code, text, sizeof(text));
    return text;
}

bool ioctl_retry(int fd, unsigned long request, void* argument) {
    int rc;
    do {
        rc = ::ioctl(fd, request, argument);
    } while (rc < 0 && errno == EINTR);
    return rc == 0;
}

struct Options {
    std::string device = "/dev/video10";
    std::string lan_host;
    std::uint16_t lan_port = 48'527;
    std::string lan_pin;
    std::string hw_decode = "auto";
    std::size_t frames = 0;
    int timeout_ms = 30'000;
    bool horizontal_flip = true;
    bool vertical_flip = false;
    amb::video::Rotation rotation = amb::video::Rotation::None;
    bool control_stdin = false;
};

void usage(const char* argv0) {
    std::cout
        << "Usage: " << argv0
        << " [--device /dev/videoX] [--lan HOST --pin 123456 [--port 48527]]"
           " [--hw-decode auto|off|vaapi|cuda] [--frames N] [--timeout-ms N]"
           " [--rotate 0|90|180|270] [--no-horizontal-flip] [--vertical-flip]"
           " [--control-stdin]\n\n"
        << "Receives H.264 over USB AOA or LAN TCP, decodes it, and writes YUYV frames to a\n"
        << "V4L2 output device. --frames 0 (the default) runs until interrupted.\n"
        << "Horizontal mirror correction is enabled by default.\n"
        << "Rotation is clockwise and defaults to 0 degrees. V4L2 size stays fixed;\n"
        << "90/270 degree output is fitted with black sidebars.\n"
        << "Vertical flip is disabled by default.\n"
        << "--control-stdin accepts lines like '90 1 0'"
           " (degrees, horizontal flip, vertical flip).\n"
        << "Start the G1 camera from the phone UI after this command is waiting.\n";
}

bool parse_nonnegative(const std::string& text, std::size_t& out) {
    unsigned long long value = 0;
    const auto [ptr, ec] = std::from_chars(text.data(), text.data() + text.size(), value);
    if (ec != std::errc{} || ptr != text.data() + text.size()) return false;
    out = static_cast<std::size_t>(value);
    return true;
}

bool parse_rotation(const std::string& text, amb::video::Rotation& rotation) {
    if (text == "0") rotation = amb::video::Rotation::None;
    else if (text == "90") rotation = amb::video::Rotation::Clockwise90;
    else if (text == "180") rotation = amb::video::Rotation::Clockwise180;
    else if (text == "270") rotation = amb::video::Rotation::Clockwise270;
    else return false;
    return true;
}

int rotation_degrees(amb::video::Rotation rotation) {
    return static_cast<int>(rotation);
}

class StdinControl {
  public:
    bool enable(std::string& error) {
        const int flags = fcntl(STDIN_FILENO, F_GETFL);
        if (flags < 0 || fcntl(STDIN_FILENO, F_SETFL, flags | O_NONBLOCK) < 0) {
            error = "unable to make control stdin nonblocking: " +
                    std::string(std::strerror(errno));
            return false;
        }
        enabled_ = true;
        return true;
    }

    bool poll(amb::video::Rotation& rotation, bool& horizontal_flip,
              bool& vertical_flip) {
        if (!enabled_) return false;
        char chunk[256];
        while (true) {
            const ssize_t count = ::read(STDIN_FILENO, chunk, sizeof(chunk));
            if (count == 0) {
                enabled_ = false;
                break;
            }
            if (count < 0) {
                if (errno == EINTR) continue;
                if (errno == EAGAIN || errno == EWOULDBLOCK) break;
                std::cerr << "control stdin read failed: " << std::strerror(errno) << "\n";
                enabled_ = false;
                break;
            }
            pending_.append(chunk, static_cast<std::size_t>(count));
            if (pending_.size() > 4096) {
                std::cerr << "control stdin line too long\n";
                pending_.clear();
            }
        }
        bool changed = false;
        std::size_t end;
        while ((end = pending_.find('\n')) != std::string::npos) {
            const std::string line = pending_.substr(0, end);
            pending_.erase(0, end + 1);
            const auto settings = amb::video::parse_transform_settings(line);
            if (!settings) {
                std::cerr << "invalid transform control line\n";
                continue;
            }
            rotation = settings->rotation;
            horizontal_flip = settings->horizontal_flip;
            vertical_flip = settings->vertical_flip;
            changed = true;
        }
        return changed;
    }

  private:
    bool enabled_ = false;
    std::string pending_;
};

class V4l2Output {
  public:
    ~V4l2Output() {
        if (sws_to_yuyv_) sws_freeContext(sws_to_yuyv_);
        if (sws_to_rgb_) sws_freeContext(sws_to_rgb_);
        if (sws_rgb_to_yuyv_) sws_freeContext(sws_rgb_to_yuyv_);
        if (fd_ >= 0) ::close(fd_);
    }

    bool open(const std::string& path, int source_width, int source_height, int fps,
              AVPixelFormat source_format, amb::video::Rotation rotation,
              bool horizontal_flip, bool vertical_flip, std::string& error) {
        const int output_width = source_width;
        const int output_height = source_height;
        if ((output_width % 2) != 0) {
            error = "YUYV output width must be even";
            return false;
        }
        fd_ = ::open(path.c_str(), O_WRONLY | O_CLOEXEC);
        if (fd_ < 0) {
            error = "open " + path + ": " + std::strerror(errno);
            return false;
        }

        v4l2_capability capability{};
        if (!ioctl_retry(fd_, VIDIOC_QUERYCAP, &capability)) {
            error = "VIDIOC_QUERYCAP: " + std::string(std::strerror(errno));
            return false;
        }
        const std::uint32_t caps = (capability.capabilities & V4L2_CAP_DEVICE_CAPS)
                                       ? capability.device_caps
                                       : capability.capabilities;
        if ((caps & V4L2_CAP_VIDEO_OUTPUT) == 0 || (caps & V4L2_CAP_READWRITE) == 0) {
            error = path + " does not support V4L2 video output via write()";
            return false;
        }

        v4l2_format format{};
        format.type = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        format.fmt.pix.width = static_cast<std::uint32_t>(output_width);
        format.fmt.pix.height = static_cast<std::uint32_t>(output_height);
        format.fmt.pix.pixelformat = V4L2_PIX_FMT_YUYV;
        format.fmt.pix.field = V4L2_FIELD_NONE;
        format.fmt.pix.colorspace = V4L2_COLORSPACE_REC709;
        format.fmt.pix.ycbcr_enc = V4L2_YCBCR_ENC_709;
        format.fmt.pix.quantization = V4L2_QUANTIZATION_LIM_RANGE;
        format.fmt.pix.xfer_func = V4L2_XFER_FUNC_709;
        if (!ioctl_retry(fd_, VIDIOC_S_FMT, &format)) {
            error = "VIDIOC_S_FMT: " + std::string(std::strerror(errno));
            return false;
        }
        if (format.fmt.pix.width != static_cast<std::uint32_t>(output_width) ||
            format.fmt.pix.height != static_cast<std::uint32_t>(output_height) ||
            format.fmt.pix.pixelformat != V4L2_PIX_FMT_YUYV) {
            error = "V4L2 device rejected requested YUYV dimensions";
            return false;
        }

        v4l2_streamparm parameters{};
        parameters.type = V4L2_BUF_TYPE_VIDEO_OUTPUT;
        parameters.parm.output.timeperframe.numerator = 1;
        parameters.parm.output.timeperframe.denominator = static_cast<std::uint32_t>(fps);
        if (!ioctl_retry(fd_, VIDIOC_S_PARM, &parameters)) {
            error = "VIDIOC_S_PARM: " + std::string(std::strerror(errno));
            return false;
        }

        source_width_ = source_width;
        source_height_ = source_height;
        width_ = output_width;
        height_ = output_height;
        rotation_ = rotation;
        horizontal_flip_ = horizontal_flip;
        vertical_flip_ = vertical_flip;
        stride_ = std::max<std::size_t>(format.fmt.pix.bytesperline,
                                        static_cast<std::size_t>(width_) * 2);
        const std::size_t minimum_size = stride_ * static_cast<std::size_t>(height_);
        const std::size_t frame_size = std::max<std::size_t>(format.fmt.pix.sizeimage,
                                                              minimum_size);
        output_.assign(frame_size, 0);
        source_format_ = source_format;
        sws_to_yuyv_ = sws_getContext(source_width_, source_height_, source_format_,
                                      width_, height_, AV_PIX_FMT_YUYV422,
                                      SWS_BILINEAR, nullptr, nullptr, nullptr);
        if (!sws_to_yuyv_) {
            error = "unable to create YUYV conversion context";
            return false;
        }
        std::cout << "V4L2 " << path << " " << width_ << "x" << height_
                  << " @" << fps << " YUYV stride=" << stride_
                  << " rotation=" << rotation_degrees(rotation_)
                  << " horizontal_flip=" << (horizontal_flip_ ? "on" : "off")
                  << " vertical_flip=" << (vertical_flip_ ? "on" : "off") << "\n";
        return true;
    }

    void set_transform(amb::video::Rotation rotation, bool horizontal_flip,
                       bool vertical_flip) {
        rotation_ = rotation;
        horizontal_flip_ = horizontal_flip;
        vertical_flip_ = vertical_flip;
        report_transform_ = true;
    }

    bool write_frame(const AVFrame* frame, std::string& error) {
        if (frame->width != source_width_ || frame->height != source_height_) {
            error = "decoded frame dimensions changed unexpectedly";
            return false;
        }
        std::uint8_t* destination[4]{output_.data(), nullptr, nullptr, nullptr};
        int destination_stride[4]{static_cast<int>(stride_), 0, 0, 0};
        if (rotation_ == amb::video::Rotation::None) {
            const int rows = sws_scale(sws_to_yuyv_, frame->data, frame->linesize, 0,
                                       source_height_, destination, destination_stride);
            if (rows != height_) {
                error = "pixel conversion produced " + std::to_string(rows) + " rows";
                return false;
            }
        } else {
            const int rotated_width = static_cast<int>(amb::video::rotated_width(
                static_cast<std::size_t>(source_width_),
                static_cast<std::size_t>(source_height_), rotation_));
            const int rotated_height = static_cast<int>(amb::video::rotated_height(
                static_cast<std::size_t>(source_width_),
                static_cast<std::size_t>(source_height_), rotation_));
            const auto rect = amb::video::fit_yuyv_rect(
                static_cast<std::size_t>(rotated_width),
                static_cast<std::size_t>(rotated_height),
                static_cast<std::size_t>(width_), static_cast<std::size_t>(height_));
            if (!rect) {
                error = "unable to fit rotated frame in YUYV output";
                return false;
            }
            rgb_source_stride_ = static_cast<std::size_t>(source_width_) * 3;
            rgb_rotated_stride_ = static_cast<std::size_t>(rotated_width) * 3;
            rgb_source_.resize(rgb_source_stride_ * static_cast<std::size_t>(source_height_));
            rgb_rotated_.resize(rgb_rotated_stride_ * static_cast<std::size_t>(rotated_height));
            if (!sws_to_rgb_) {
                sws_to_rgb_ = sws_getContext(source_width_, source_height_, source_format_,
                                             source_width_, source_height_, AV_PIX_FMT_RGB24,
                                             SWS_BILINEAR, nullptr, nullptr, nullptr);
                if (!sws_to_rgb_) {
                    error = "unable to create RGB conversion context";
                    return false;
                }
            }
            sws_rgb_to_yuyv_ = sws_getCachedContext(
                sws_rgb_to_yuyv_, rotated_width, rotated_height, AV_PIX_FMT_RGB24,
                static_cast<int>(rect->width), static_cast<int>(rect->height),
                AV_PIX_FMT_YUYV422, SWS_BILINEAR, nullptr, nullptr, nullptr);
            if (!sws_rgb_to_yuyv_) {
                error = "unable to create rotated YUYV conversion context";
                return false;
            }
            std::uint8_t* rgb_destination[4]{rgb_source_.data(), nullptr, nullptr, nullptr};
            int rgb_destination_stride[4]{static_cast<int>(rgb_source_stride_), 0, 0, 0};
            const int rgb_rows = sws_scale(sws_to_rgb_, frame->data, frame->linesize, 0,
                                           source_height_, rgb_destination,
                                           rgb_destination_stride);
            if (rgb_rows != source_height_) {
                error = "RGB conversion produced " + std::to_string(rgb_rows) + " rows";
                return false;
            }
            if (!amb::video::rotate_rgb24(rgb_source_,
                                          static_cast<std::size_t>(source_width_),
                                          static_cast<std::size_t>(source_height_),
                                          rgb_source_stride_, rgb_rotated_,
                                          rgb_rotated_stride_, rotation_)) {
                error = "unable to rotate RGB frame";
                return false;
            }
            std::fill(output_.begin(), output_.end(), 128);
            for (int y = 0; y < height_; ++y) {
                auto* row = output_.data() + static_cast<std::size_t>(y) * stride_;
                for (int x = 0; x < width_ * 2; x += 2) row[x] = 16;
            }
            std::uint8_t* fitted_output = output_.data() + rect->y * stride_ + rect->x * 2;
            std::uint8_t* fitted_destination[4]{fitted_output, nullptr, nullptr, nullptr};
            const std::uint8_t* rotated_source[4]{rgb_rotated_.data(), nullptr, nullptr, nullptr};
            int rotated_stride[4]{static_cast<int>(rgb_rotated_stride_), 0, 0, 0};
            const int rows = sws_scale(sws_rgb_to_yuyv_, rotated_source, rotated_stride, 0,
                                       rotated_height, fitted_destination, destination_stride);
            if (rows != static_cast<int>(rect->height)) {
                error = "rotated YUYV conversion produced " + std::to_string(rows) + " rows";
                return false;
            }
        }
        if (horizontal_flip_ &&
            !amb::video::flip_yuyv422_horizontal(output_,
                                                  static_cast<std::size_t>(width_),
                                                  static_cast<std::size_t>(height_), stride_)) {
            error = "unable to apply horizontal mirror correction";
            return false;
        }
        if (vertical_flip_ &&
            !amb::video::flip_yuyv422_vertical(output_,
                                                static_cast<std::size_t>(width_),
                                                static_cast<std::size_t>(height_), stride_)) {
            error = "unable to apply vertical flip";
            return false;
        }

        ssize_t written;
        do {
            written = ::write(fd_, output_.data(), output_.size());
        } while (written < 0 && errno == EINTR);
        if (written < 0) {
            error = "V4L2 write: " + std::string(std::strerror(errno));
            return false;
        }
        if (static_cast<std::size_t>(written) != output_.size()) {
            error = "V4L2 accepted a partial frame";
            return false;
        }
        if (report_transform_) {
            std::cout << "transform rotation=" << rotation_degrees(rotation_)
                      << " horizontal_flip=" << (horizontal_flip_ ? "on" : "off")
                      << " vertical_flip=" << (vertical_flip_ ? "on" : "off") << "\n";
            report_transform_ = false;
        }
        return true;
    }

  private:
    int fd_ = -1;
    int source_width_ = 0;
    int source_height_ = 0;
    int width_ = 0;
    int height_ = 0;
    std::size_t stride_ = 0;
    AVPixelFormat source_format_ = AV_PIX_FMT_NONE;
    std::size_t rgb_source_stride_ = 0;
    std::size_t rgb_rotated_stride_ = 0;
    amb::video::Rotation rotation_ = amb::video::Rotation::None;
    bool horizontal_flip_ = true;
    bool vertical_flip_ = false;
    bool report_transform_ = false;
    SwsContext* sws_to_yuyv_ = nullptr;
    SwsContext* sws_to_rgb_ = nullptr;
    SwsContext* sws_rgb_to_yuyv_ = nullptr;
    std::vector<std::uint8_t> output_;
    std::vector<std::uint8_t> rgb_source_;
    std::vector<std::uint8_t> rgb_rotated_;
};

struct CodecContextDeleter {
    void operator()(AVCodecContext* context) const { avcodec_free_context(&context); }
};
struct PacketDeleter {
    void operator()(AVPacket* packet) const { av_packet_free(&packet); }
};
struct FrameDeleter {
    void operator()(AVFrame* frame) const { av_frame_free(&frame); }
};

}  // namespace

int main(int argc, char** argv) {
    Options options{};
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--device" && i + 1 < argc) {
            options.device = argv[++i];
        } else if (arg == "--lan" && i + 1 < argc) {
            options.lan_host = argv[++i];
        } else if (arg == "--pin" && i + 1 < argc) {
            options.lan_pin = argv[++i];
        } else if (arg == "--port" && i + 1 < argc) {
            std::size_t parsed = 0;
            if (!parse_nonnegative(argv[++i], parsed) || parsed == 0 || parsed > 65'535) {
                std::cerr << "invalid --port\n";
                return 2;
            }
            options.lan_port = static_cast<std::uint16_t>(parsed);
        } else if (arg == "--hw-decode" && i + 1 < argc) {
            options.hw_decode = argv[++i];
        } else if (arg == "--frames" && i + 1 < argc) {
            if (!parse_nonnegative(argv[++i], options.frames)) {
                std::cerr << "invalid --frames\n";
                return 2;
            }
        } else if (arg == "--timeout-ms" && i + 1 < argc) {
            std::size_t parsed = 0;
            if (!parse_nonnegative(argv[++i], parsed) || parsed == 0 || parsed > 600'000) {
                std::cerr << "invalid --timeout-ms\n";
                return 2;
            }
            options.timeout_ms = static_cast<int>(parsed);
        } else if (arg == "--no-horizontal-flip") {
            options.horizontal_flip = false;
        } else if (arg == "--vertical-flip") {
            options.vertical_flip = true;
        } else if (arg == "--rotate" && i + 1 < argc) {
            if (!parse_rotation(argv[++i], options.rotation)) {
                std::cerr << "invalid --rotate; expected 0, 90, 180, or 270\n";
                return 2;
            }
        } else if (arg == "--control-stdin") {
            options.control_stdin = true;
        } else if (arg == "-h" || arg == "--help") {
            usage(argv[0]);
            return 0;
        } else {
            std::cerr << "unknown or incomplete argument: " << arg << "\n";
            usage(argv[0]);
            return 2;
        }
    }

    if (!options.lan_host.empty()) {
        if (options.lan_pin.size() != 6 ||
            !std::all_of(options.lan_pin.begin(), options.lan_pin.end(), ::isdigit)) {
            std::cerr << "--lan requires a six-digit --pin\n";
            return 2;
        }
    } else if (!options.lan_pin.empty()) {
        std::cerr << "--pin requires --lan\n";
        return 2;
    }

    StdinControl control;
    if (options.control_stdin) {
        std::cout.setf(std::ios::unitbuf);
        std::string error;
        if (!control.enable(error)) {
            std::cerr << error << "\n";
            return 2;
        }
    }

    const AVCodec* codec = avcodec_find_decoder(AV_CODEC_ID_H264);
    if (!codec) {
        std::cerr << "H.264 decoder is unavailable\n";
        return 3;
    }
    std::unique_ptr<AVCodecContext, CodecContextDeleter> decoder(
        avcodec_alloc_context3(codec));
    std::unique_ptr<AVPacket, PacketDeleter> packet(av_packet_alloc());
    std::unique_ptr<AVFrame, FrameDeleter> frame(av_frame_alloc());
    std::unique_ptr<AVFrame, FrameDeleter> transfer_frame(av_frame_alloc());
    if (!decoder || !packet || !frame || !transfer_frame) {
        std::cerr << "unable to allocate FFmpeg decoder state\n";
        return 3;
    }
    decoder->pkt_timebase = AVRational{1, 1'000'000};
    amb::HardwareDecode hardware_decode;
    std::string active_decode;
    std::string decode_error;
    if (!hardware_decode.configure(decoder.get(), codec, options.hw_decode,
                                   active_decode, decode_error)) {
        std::cerr << "hardware decode setup failed: " << decode_error << "\n";
        return 3;
    }
    int rc = avcodec_open2(decoder.get(), codec, nullptr);
    if (rc < 0 && options.hw_decode == "auto" && active_decode != "software") {
        std::cerr << "hardware decoder open failed; falling back to software: "
                  << av_error(rc) << "\n";
        decoder.reset(avcodec_alloc_context3(codec));
        if (!decoder) return 3;
        decoder->pkt_timebase = AVRational{1, 1'000'000};
        active_decode = "software";
        rc = avcodec_open2(decoder.get(), codec, nullptr);
    }
    if (rc < 0) {
        std::cerr << "avcodec_open2: " << av_error(rc) << "\n";
        return 3;
    }
    std::cout << "decoder=" << active_decode << "\n";

    libusb_context* usb_context = nullptr;
    std::unique_ptr<amb::AccessoryDevice> accessory;
    amb::TcpConnection tcp;
    std::string error;
    if (options.lan_host.empty()) {
        rc = libusb_init(&usb_context);
        if (rc < 0) {
            std::cerr << "libusb_init: " << libusb_error_name(rc) << "\n";
            return 3;
        }
        accessory = std::make_unique<amb::AccessoryDevice>(usb_context);
        if (!accessory->connect(std::nullopt, error)) {
            std::cerr << "accessory connect failed: " << error << "\n";
            libusb_exit(usb_context);
            return 4;
        }
        std::cout << "transport=usb " << amb::aoa::describe(accessory->id()) << "\n";
    } else {
        if (!tcp.connect(options.lan_host, options.lan_port, options.lan_pin,
                         5'000, error)) {
            std::cerr << "LAN connect failed: " << error << "\n";
            return 4;
        }
        std::cout << "transport=lan endpoint=" << options.lan_host << ":"
                  << options.lan_port << "\n";
    }

    const auto send_frame = [&](amb::wire::Type type, std::uint16_t flags,
                                std::uint32_t sequence, std::uint64_t pts_us,
                                std::span<const std::uint8_t> payload, int timeout_ms,
                                std::string& send_error) {
        if (accessory) {
            return accessory->send_frame(type, flags, sequence, pts_us, payload,
                                         timeout_ms, send_error);
        }
        return tcp.send_frame(type, flags, sequence, pts_us, payload, timeout_ms, send_error);
    };
    const auto read_frame = [&](amb::wire::Header& header,
                                std::vector<std::uint8_t>& payload, int timeout_ms,
                                std::string& read_error) {
        if (accessory) return accessory->read_frame(header, payload, timeout_ms, read_error);
        return tcp.read_frame(header, payload, timeout_ms, read_error);
    };

    int result = 0;
    {
            if (!send_frame(amb::wire::Type::VideoIdrRequest, 0, 2, 0,
                            {}, 2000, error)) {
                std::cerr << "warning: initial IDR request failed; waiting for Android: "
                          << error << "\n";
                error.clear();
            }

            V4l2Output output;
            bool configured = false;
            bool started = false;
            int width = 0;
            int height = 0;
            int fps = 0;
            std::size_t written_frames = 0;
            std::size_t discontinuities = 0;
            std::uint64_t last_pts_us = 0;
            auto first_wall = std::chrono::steady_clock::time_point{};

            while (result == 0 &&
                   (options.frames == 0 || written_frames < options.frames)) {
                amb::wire::Header header{};
                std::vector<std::uint8_t> payload;
                if (!read_frame(header, payload, options.timeout_ms, error)) {
                    std::cerr << "read failed after " << written_frames
                              << " output frames: " << error << "\n";
                    result = 6;
                    break;
                }

                if (header.type == amb::wire::Type::VideoConfig) {
                    if (payload.size() != 16 || payload[12] != 1 || payload[13] != 1) {
                        std::cerr << "invalid VIDEO_CONFIG\n";
                        result = 7;
                        break;
                    }
                    const int next_width = get_u16(payload.data());
                    const int next_height = get_u16(payload.data() + 2);
                    const int next_fps = get_u16(payload.data() + 4);
                    constexpr int kMaximumDimension = 8192;
                    constexpr std::uint64_t kMaximumPixels = 33'554'432;
                    if (next_width <= 0 || next_height <= 0 || next_fps <= 0 ||
                        next_width > kMaximumDimension || next_height > kMaximumDimension ||
                        static_cast<std::uint64_t>(next_width) * next_height > kMaximumPixels) {
                        std::cerr << "invalid VIDEO_CONFIG dimensions or rate\n";
                        result = 7;
                        break;
                    }
                    if (configured &&
                        (width != next_width || height != next_height || fps != next_fps)) {
                        std::cerr << "VIDEO_CONFIG changed during active output\n";
                        result = 7;
                        break;
                    }
                    width = next_width;
                    height = next_height;
                    fps = next_fps;
                    configured = true;
                    std::cout << "VIDEO_CONFIG " << width << "x" << height
                              << " @" << fps << " H.264 Annex-B\n";
                    continue;
                }
                if (header.type != amb::wire::Type::VideoAu) continue;
                if (!configured || payload.empty()) continue;

                const bool keyframe = (header.flags & amb::wire::kVideoFlagKeyframe) != 0;
                const bool config_included =
                    (header.flags & amb::wire::kVideoFlagConfigIncluded) != 0;
                const bool discontinuity =
                    (header.flags & amb::wire::kVideoFlagDiscontinuity) != 0;
                if (!started && (!keyframe || !config_included)) continue;
                if (started && header.pts_us <= last_pts_us) {
                    std::cerr << "non-monotonic VIDEO_AU PTS\n";
                    result = 8;
                    break;
                }
                if (discontinuity) {
                    avcodec_flush_buffers(decoder.get());
                    if (started) ++discontinuities;
                }

                av_packet_unref(packet.get());
                if (payload.size() > static_cast<std::size_t>(std::numeric_limits<int>::max())) {
                    std::cerr << "VIDEO_AU is too large for FFmpeg\n";
                    result = 8;
                    break;
                }
                rc = av_new_packet(packet.get(), static_cast<int>(payload.size()));
                if (rc < 0) {
                    std::cerr << "av_new_packet: " << av_error(rc) << "\n";
                    result = 9;
                    break;
                }
                std::memcpy(packet->data, payload.data(), payload.size());
                packet->pts = static_cast<std::int64_t>(header.pts_us);
                packet->dts = packet->pts;
                if (keyframe) packet->flags |= AV_PKT_FLAG_KEY;

                rc = avcodec_send_packet(decoder.get(), packet.get());
                if (rc < 0) {
                    std::cerr << "avcodec_send_packet: " << av_error(rc) << "\n";
                    result = 9;
                    break;
                }
                last_pts_us = header.pts_us;

                while (result == 0) {
                    rc = avcodec_receive_frame(decoder.get(), frame.get());
                    if (rc == AVERROR(EAGAIN) || rc == AVERROR_EOF) break;
                    if (rc < 0) {
                        std::cerr << "avcodec_receive_frame: " << av_error(rc) << "\n";
                        result = 9;
                        break;
                    }
                    AVFrame* display_frame = hardware_decode.system_frame(
                        frame.get(), transfer_frame.get(), error);
                    if (display_frame == nullptr) {
                        std::cerr << error << "\n";
                        result = 9;
                        break;
                    }
                    if (control.poll(options.rotation, options.horizontal_flip,
                                     options.vertical_flip) && started) {
                        output.set_transform(options.rotation, options.horizontal_flip,
                                             options.vertical_flip);
                    }
                    if (!started) {
                        if (display_frame->width != width || display_frame->height != height) {
                            std::cerr << "decoded dimensions do not match VIDEO_CONFIG\n";
                            result = 9;
                            break;
                        }
                        if (!output.open(options.device, width, height, fps,
                                         static_cast<AVPixelFormat>(display_frame->format),
                                         options.rotation, options.horizontal_flip,
                                         options.vertical_flip, error)) {
                            std::cerr << "V4L2 setup failed: " << error << "\n";
                            result = 10;
                            break;
                        }
                        started = true;
                        first_wall = std::chrono::steady_clock::now();
                    }
                    if (!output.write_frame(display_frame, error)) {
                        std::cerr << "V4L2 output failed: " << error << "\n";
                        result = 10;
                        break;
                    }
                    ++written_frames;
                    if (written_frames == 1 || written_frames % 30 == 0) {
                        std::cout << "frames=" << written_frames
                                  << " discontinuities=" << discontinuities << "\n";
                    }
                    if (options.frames != 0 && written_frames >= options.frames) break;
                }
            }

            if (result == 0 && options.frames != 0) {
                const double wall_seconds = std::chrono::duration<double>(
                    std::chrono::steady_clock::now() - first_wall).count();
                std::cout << "[PASS G3 V4L2 slice] frames=" << written_frames
                          << " discontinuities=" << discontinuities
                          << " wall_s=" << wall_seconds
                          << " device=" << options.device << "\n";
            }
    }
    // AccessoryDevice owns a libusb handle. Close it before tearing down the
    // context; otherwise timeout/error exits can destroy the handle after
    // libusb_exit() and crash during process cleanup.
    accessory.reset();
    if (usb_context != nullptr) libusb_exit(usb_context);
    return result;
}
