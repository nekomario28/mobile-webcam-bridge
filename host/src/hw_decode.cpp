#include "amb/hw_decode.hpp"

#include <array>

namespace amb {

HardwareDecode::~HardwareDecode() {
    av_buffer_unref(&device_);
}

AVPixelFormat HardwareDecode::select_format(AVCodecContext* context,
                                            const AVPixelFormat* formats) {
    const auto* self = static_cast<HardwareDecode*>(context->opaque);
    for (const AVPixelFormat* format = formats; *format != AV_PIX_FMT_NONE; ++format) {
        if (self != nullptr && *format == self->hardware_format_) return *format;
    }
    return formats[0];
}

bool HardwareDecode::try_device(AVCodecContext* context, const AVCodec* codec,
                                AVHWDeviceType type, std::string& error) {
    AVPixelFormat format = AV_PIX_FMT_NONE;
    for (int index = 0;; ++index) {
        const AVCodecHWConfig* config = avcodec_get_hw_config(codec, index);
        if (config == nullptr) break;
        if ((config->methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX) != 0 &&
            config->device_type == type) {
            format = config->pix_fmt;
            break;
        }
    }
    if (format == AV_PIX_FMT_NONE) {
        error = "decoder exposes no compatible hardware configuration";
        return false;
    }

    AVBufferRef* device = nullptr;
    const int rc = av_hwdevice_ctx_create(&device, type, nullptr, nullptr, 0);
    if (rc < 0) {
        char message[AV_ERROR_MAX_STRING_SIZE]{};
        av_strerror(rc, message, sizeof(message));
        error = message;
        return false;
    }

    av_buffer_unref(&device_);
    device_ = device;
    hardware_format_ = format;
    context->opaque = this;
    context->get_format = &HardwareDecode::select_format;
    context->hw_device_ctx = av_buffer_ref(device_);
    return context->hw_device_ctx != nullptr;
}

bool HardwareDecode::configure(AVCodecContext* context, const AVCodec* codec,
                               const std::string& requested, std::string& active,
                               std::string& error) {
    active = "software";
    if (requested == "off" || requested == "software") return true;

    if (requested == "auto") {
        constexpr std::array candidates{
            AV_HWDEVICE_TYPE_VAAPI,
            AV_HWDEVICE_TYPE_CUDA,
        };
        for (const auto type : candidates) {
            std::string candidate_error;
            if (try_device(context, codec, type, candidate_error)) {
                active = av_hwdevice_get_type_name(type);
                return true;
            }
        }
        return true;
    }

    const AVHWDeviceType type = av_hwdevice_find_type_by_name(requested.c_str());
    if (type == AV_HWDEVICE_TYPE_NONE) {
        error = "unknown hardware decoder: " + requested;
        return false;
    }
    if (!try_device(context, codec, type, error)) return false;
    active = av_hwdevice_get_type_name(type);
    return true;
}

AVFrame* HardwareDecode::system_frame(AVFrame* decoded, AVFrame* transfer,
                                      std::string& error) {
    if (hardware_format_ == AV_PIX_FMT_NONE || decoded->format != hardware_format_) return decoded;
    av_frame_unref(transfer);
    const int rc = av_hwframe_transfer_data(transfer, decoded, 0);
    if (rc < 0) {
        char message[AV_ERROR_MAX_STRING_SIZE]{};
        av_strerror(rc, message, sizeof(message));
        error = std::string("hardware frame transfer failed: ") + message;
        return nullptr;
    }
    av_frame_copy_props(transfer, decoded);
    return transfer;
}

}  // namespace amb
