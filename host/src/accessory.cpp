#include "amb/accessory.hpp"
#include "amb/wire.hpp"

#include <libusb-1.0/libusb.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <iostream>
#include <limits>
#include <numeric>
#include <span>
#include <vector>

namespace amb {
namespace {

bool id_matches(const aoa::DeviceId& id, const std::optional<aoa::DeviceId>& target) {
    if (!target) return true;
    if (target->bus != 0 && id.bus != target->bus) return false;
    if (target->address != 0 && id.address != target->address) return false;
    if (target->vid != 0 && id.vid != target->vid) return false;
    if (target->pid != 0 && id.pid != target->pid) return false;
    return true;
}

aoa::DeviceId read_id(libusb_device* dev) {
    libusb_device_descriptor desc{};
    if (libusb_get_device_descriptor(dev, &desc) != 0) return {};
    return {
        .bus = libusb_get_bus_number(dev),
        .address = libusb_get_device_address(dev),
        .vid = desc.idVendor,
        .pid = desc.idProduct,
    };
}

std::uint64_t now_us() {
    return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::microseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count());
}

std::uint64_t challenge_for(std::uint32_t sequence) {
    std::uint64_t x = static_cast<std::uint64_t>(sequence) + 0x9e3779b97f4a7c15ULL;
    x = (x ^ (x >> 30)) * 0xbf58476d1ce4e5b9ULL;
    x = (x ^ (x >> 27)) * 0x94d049bb133111ebULL;
    return x ^ (x >> 31);
}

int remaining_timeout_ms(std::chrono::steady_clock::time_point deadline) {
    const auto now = std::chrono::steady_clock::now();
    if (now >= deadline) return 0;
    const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - now);
    return std::max(1, static_cast<int>(remaining.count()));
}

std::uint32_t session_sequence_base() {
    // The Android side echoes sequence numbers; it does not require them to start at 1.
    // Derive a non-zero per-process base from the monotonic clock so a late PONG left in
    // the USB IN endpoint by a previous host process cannot be mistaken for the first
    // response of a new smoke-test session.
    const auto t = now_us();
    std::uint32_t base = static_cast<std::uint32_t>(t ^ (t >> 32));
    base ^= 0xa5b35705U;
    if (base == 0) base = 1;
    return base;
}

}  // namespace

AccessoryDevice::AccessoryDevice(libusb_context* ctx) : ctx_(ctx) {}
AccessoryDevice::~AccessoryDevice() { disconnect(); }

bool AccessoryDevice::connect(std::optional<aoa::DeviceId> target, std::string& error) {
    disconnect();
    libusb_device** list = nullptr;
    const auto count = libusb_get_device_list(ctx_, &list);
    if (count < 0) {
        error = std::string("libusb_get_device_list: ") + libusb_error_name(static_cast<int>(count));
        return false;
    }

    libusb_device* selected = nullptr;
    for (ssize_t i = 0; i < count; ++i) {
        const auto id = read_id(list[i]);
        if (id.vid == aoa::kGoogleVid && aoa::is_accessory_pid(id.pid) &&
            id_matches(id, target)) {
            selected = libusb_ref_device(list[i]);
            id_ = id;
            break;
        }
    }
    libusb_free_device_list(list, 1);

    if (!selected) {
        error = "no matching AOA accessory device found";
        return false;
    }

    int rc = libusb_open(selected, &handle_);
    if (rc < 0) {
        error = std::string("libusb_open accessory: ") + libusb_error_name(rc);
        libusb_unref_device(selected);
        handle_ = nullptr;
        return false;
    }

    libusb_config_descriptor* config = nullptr;
    rc = libusb_get_active_config_descriptor(selected, &config);
    libusb_unref_device(selected);
    if (rc < 0 || !config) {
        error = std::string("get active USB config: ") + libusb_error_name(rc);
        disconnect();
        return false;
    }

    int fallback_interface = -1;
    std::uint8_t fallback_in = 0;
    std::uint8_t fallback_out = 0;
    std::uint16_t fallback_in_packet_size = 0;
    bool found_preferred = false;

    for (std::uint8_t i = 0; i < config->bNumInterfaces && !found_preferred; ++i) {
        const auto& iface = config->interface[i];
        for (int a = 0; a < iface.num_altsetting; ++a) {
            const auto& alt = iface.altsetting[a];
            std::uint8_t ep_in = 0;
            std::uint8_t ep_out = 0;
            std::uint16_t ep_in_packet_size = 0;
            for (std::uint8_t e = 0; e < alt.bNumEndpoints; ++e) {
                const auto& ep = alt.endpoint[e];
                if ((ep.bmAttributes & LIBUSB_TRANSFER_TYPE_MASK) != LIBUSB_TRANSFER_TYPE_BULK) {
                    continue;
                }
                if ((ep.bEndpointAddress & LIBUSB_ENDPOINT_DIR_MASK) == LIBUSB_ENDPOINT_IN) {
                    ep_in = ep.bEndpointAddress;
                    ep_in_packet_size = ep.wMaxPacketSize & 0x07ffU;
                } else {
                    ep_out = ep.bEndpointAddress;
                }
            }
            if (ep_in != 0 && ep_out != 0) {
                if (fallback_interface < 0) {
                    fallback_interface = alt.bInterfaceNumber;
                    fallback_in = ep_in;
                    fallback_out = ep_out;
                    fallback_in_packet_size = ep_in_packet_size;
                }
                if (alt.bInterfaceClass == LIBUSB_CLASS_VENDOR_SPEC &&
                    alt.bInterfaceSubClass == 0xff) {
                    interface_ = alt.bInterfaceNumber;
                    in_endpoint_ = ep_in;
                    out_endpoint_ = ep_out;
                    in_packet_size_ = ep_in_packet_size;
                    found_preferred = true;
                    break;
                }
            }
        }
    }
    libusb_free_config_descriptor(config);

    if (!found_preferred && fallback_interface >= 0) {
        interface_ = fallback_interface;
        in_endpoint_ = fallback_in;
        out_endpoint_ = fallback_out;
        in_packet_size_ = fallback_in_packet_size;
    }
    if (interface_ < 0 || in_endpoint_ == 0 || out_endpoint_ == 0 ||
        in_packet_size_ == 0) {
        error = "AOA device has no bulk IN/OUT endpoint pair";
        disconnect();
        return false;
    }

    libusb_set_auto_detach_kernel_driver(handle_, 1);
    rc = libusb_claim_interface(handle_, interface_);
    if (rc < 0) {
        error = std::string("claim AOA interface: ") + libusb_error_name(rc);
        disconnect();
        return false;
    }

    return true;
}

void AccessoryDevice::disconnect() {
    if (handle_) {
        if (interface_ >= 0) libusb_release_interface(handle_, interface_);
        libusb_close(handle_);
    }
    handle_ = nullptr;
    interface_ = -1;
    in_endpoint_ = 0;
    out_endpoint_ = 0;
    in_packet_size_ = 0;
    id_ = {};
}

bool AccessoryDevice::write_all(const std::uint8_t* data, std::size_t size,
                                int timeout_ms, std::string& error) {
    std::size_t offset = 0;
    while (offset < size) {
        const auto remaining = size - offset;
        const auto chunk = static_cast<int>(std::min<std::size_t>(
            remaining, static_cast<std::size_t>(std::numeric_limits<int>::max())));
        int transferred = 0;
        const int rc = libusb_bulk_transfer(
            handle_, out_endpoint_, const_cast<unsigned char*>(data + offset), chunk,
            &transferred, static_cast<unsigned int>(timeout_ms));
        if (rc < 0) {
            error = std::string("USB bulk OUT: ") + libusb_error_name(rc);
            return false;
        }
        if (transferred <= 0) {
            error = "USB bulk OUT made no progress";
            return false;
        }
        offset += static_cast<std::size_t>(transferred);
    }
    return true;
}

bool AccessoryDevice::read_exact(std::uint8_t* data, std::size_t size,
                                 int timeout_ms, std::string& error) {
    std::size_t offset = 0;
    auto deadline = std::chrono::steady_clock::now() +
                    std::chrono::milliseconds(timeout_ms);
    while (offset < size) {
        const int read_timeout = remaining_timeout_ms(deadline);
        if (read_timeout == 0) {
            error = "USB bulk IN timed out at byte " + std::to_string(offset) +
                    " of " + std::to_string(size);
            return false;
        }
        const auto remaining = size - offset;
        const auto chunk = static_cast<int>(std::min<std::size_t>(
            remaining, static_cast<std::size_t>(std::numeric_limits<int>::max())));
        int transferred = 0;
        const int rc = libusb_bulk_transfer(
            handle_, in_endpoint_, data + offset, chunk, &transferred,
            static_cast<unsigned int>(read_timeout));
        if (rc < 0) {
            error = std::string("USB bulk IN: ") + libusb_error_name(rc) +
                    " at byte " + std::to_string(offset) + " of " +
                    std::to_string(size) + " transferred=" +
                    std::to_string(transferred);
            return false;
        }
        // A packet-aligned device write may terminate with a zero-length packet.
        // Consume it without resetting the per-read deadline.
        if (transferred == 0) continue;
        offset += static_cast<std::size_t>(transferred);
        deadline = std::chrono::steady_clock::now() +
                   std::chrono::milliseconds(timeout_ms);
    }
    return true;
}

bool AccessoryDevice::send_frame(wire::Type type, std::uint16_t flags,
                                 std::uint32_t sequence, std::uint64_t pts_us,
                                 std::span<const std::uint8_t> payload,
                                 int timeout_ms, std::string& error) {
    if (!connected()) {
        error = "AOA accessory is not connected";
        return false;
    }
    const auto frame = wire::make_frame(type, flags, sequence, pts_us, payload);
    if (frame.empty() && !payload.empty()) {
        error = "failed to construct AMB frame";
        return false;
    }

    // Android's UsbAccessory stream API requires one read to consume an entire
    // USB transfer; unread bytes from that transfer are discarded. The Android
    // peer reads the fixed header and payload separately, so preserve those as
    // separate bulk transfers instead of coalescing the whole AMB frame into one.
    if (!write_all(frame.data(), wire::kHeaderSize, timeout_ms, error)) {
        return false;
    }
    if (frame.size() == wire::kHeaderSize) return true;
    return write_all(frame.data() + wire::kHeaderSize,
                     frame.size() - wire::kHeaderSize,
                     timeout_ms, error);
}

bool AccessoryDevice::read_frame(wire::Header& header,
                                 std::vector<std::uint8_t>& payload,
                                 int timeout_ms, std::string& error) {
    if (!connected()) {
        error = "AOA accessory is not connected";
        return false;
    }
    // The Android peer writes every header and payload as separate USB
    // transfers. Read headers with a max-packet-sized buffer: libusb documents
    // that this avoids overflow when attaching after an older process left a
    // payload transfer queued. Discard those stale transfers until the next
    // valid 24-byte AMB header.
    std::array<std::uint8_t, 1024> transfer{};
    const int transfer_size = std::clamp<int>(
        in_packet_size_, static_cast<int>(wire::kHeaderSize),
        static_cast<int>(transfer.size()));
    const auto deadline = std::chrono::steady_clock::now() +
                          std::chrono::milliseconds(timeout_ms);
    std::size_t discarded_transfers = 0;
    while (true) {
        const int read_timeout = remaining_timeout_ms(deadline);
        if (read_timeout == 0) {
            error = "USB bulk IN timed out waiting for AMB header";
            return false;
        }
        int transferred = 0;
        const int rc = libusb_bulk_transfer(
            handle_, in_endpoint_, transfer.data(), transfer_size, &transferred,
            static_cast<unsigned int>(read_timeout));
        if (rc < 0) {
            error = std::string("USB bulk IN header: ") + libusb_error_name(rc);
            return false;
        }
        if (transferred == 0) continue;
        if (transferred != static_cast<int>(wire::kHeaderSize)) {
            ++discarded_transfers;
            continue;
        }
        std::string parse_error;
        const auto decoded = wire::decode_header(
            std::span<const std::uint8_t>(transfer.data(), wire::kHeaderSize), parse_error);
        if (!decoded) {
            ++discarded_transfers;
            continue;
        }
        header = *decoded;
        break;
    }
    if (discarded_transfers != 0) {
        std::cerr << "USB bulk IN resynchronized after discarding "
                  << discarded_transfers << " stale transfer(s)\n";
    }
    payload.assign(header.payload_len, 0);
    if (!payload.empty() && !read_exact(payload.data(), payload.size(), timeout_ms, error)) {
        payload.clear();
        return false;
    }
    return true;
}

bool AccessoryDevice::smoke_test(std::size_t iterations, int timeout_ms,
                                 std::string& error) {
    if (!connected()) {
        error = "AOA accessory is not connected";
        return false;
    }
    if (iterations == 0) {
        error = "smoke iteration count must be non-zero";
        return false;
    }

    std::vector<double> rtt_ms;
    rtt_ms.reserve(iterations);
    const auto sequence_base = session_sequence_base();

    for (std::size_t i = 0; i < iterations; ++i) {
        auto seq = sequence_base + static_cast<std::uint32_t>(i);
        if (seq == 0) seq = 1;
        const auto challenge = challenge_for(seq);
        const auto ping_payload = wire::encode_u64(challenge);
        const auto sent_us = now_us();
        if (!send_frame(wire::Type::Ping, 0, seq, sent_us, ping_payload,
                        timeout_ms, error)) {
            return false;
        }

        const auto deadline = std::chrono::steady_clock::now() +
                              std::chrono::milliseconds(timeout_ms);
        bool matched_pong = false;
        while (!matched_pong) {
            const int read_timeout = remaining_timeout_ms(deadline);
            if (read_timeout == 0) {
                error = "timed out waiting for PONG at sequence " + std::to_string(seq);
                return false;
            }

            wire::Header header{};
            std::vector<std::uint8_t> response;
            if (!read_frame(header, response, read_timeout, error)) return false;

            if (header.type != wire::Type::Pong || header.sequence != seq) {
                // G1 multiplexes VIDEO_CONFIG/VIDEO_AU and control responses over
                // one framed USB stream. Stale control replies from a previous
                // process can also be queued in the endpoint. Ignore any complete
                // frame that is not the response to this exact request.
                continue;
            }
            if (header.payload_len != ping_payload.size()) {
                // Treat a same-sequence malformed/stale reply as unrelated input
                // and keep waiting within the single absolute deadline.
                continue;
            }
            const auto echoed = wire::decode_u64(response);
            if (!echoed || *echoed != challenge) {
                // Sequence numbers are no longer intentionally reused between
                // sessions, but fail-open-to-wait here makes the test robust to a
                // queued reply from older builds that started every session at 1.
                continue;
            }
            matched_pong = true;
        }

        const auto done_us = now_us();
        rtt_ms.push_back(static_cast<double>(done_us - sent_us) / 1000.0);

        if (iterations >= 10 && ((i + 1) % std::max<std::size_t>(1, iterations / 10) == 0)) {
            std::cout << "[G0.5] " << (i + 1) << "/" << iterations << " verified\n";
        }
    }

    std::sort(rtt_ms.begin(), rtt_ms.end());
    const auto percentile = [&](double p) {
        const auto index = static_cast<std::size_t>(
            std::min<double>(rtt_ms.size() - 1, std::floor((rtt_ms.size() - 1) * p)));
        return rtt_ms[index];
    };
    const double mean = std::accumulate(rtt_ms.begin(), rtt_ms.end(), 0.0) /
                        static_cast<double>(rtt_ms.size());
    std::cout << "[PASS G0.5] exchanges=" << iterations
              << " sequence_base=" << sequence_base
              << " rtt_ms min=" << rtt_ms.front()
              << " avg=" << mean
              << " p50=" << percentile(0.50)
              << " p95=" << percentile(0.95)
              << " max=" << rtt_ms.back() << "\n";
    return true;
}

}  // namespace amb
