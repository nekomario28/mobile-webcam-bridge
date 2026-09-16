#pragma once

#include <string>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavutil/buffer.h>
#include <libavutil/frame.h>
#include <libavutil/hwcontext.h>
}

namespace amb {

class HardwareDecode {
  public:
    HardwareDecode() = default;
    ~HardwareDecode();

    HardwareDecode(const HardwareDecode&) = delete;
    HardwareDecode& operator=(const HardwareDecode&) = delete;

    bool configure(AVCodecContext* context, const AVCodec* codec,
                   const std::string& requested, std::string& active,
                   std::string& error);
    AVFrame* system_frame(AVFrame* decoded, AVFrame* transfer, std::string& error);

  private:
    static AVPixelFormat select_format(AVCodecContext* context,
                                       const AVPixelFormat* formats);
    bool try_device(AVCodecContext* context, const AVCodec* codec,
                    AVHWDeviceType type, std::string& error);

    AVBufferRef* device_ = nullptr;
    AVPixelFormat hardware_format_ = AV_PIX_FMT_NONE;
};

}  // namespace amb
