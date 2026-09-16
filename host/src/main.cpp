#include "amb/accessory.hpp"
#include "amb/aoa.hpp"

#include <libusb-1.0/libusb.h>

#include <charconv>
#include <iomanip>
#include <iostream>
#include <optional>
#include <sstream>
#include <string>
#include <vector>

namespace {

struct Options {
    bool list = false;
    bool do_switch = false;
    std::size_t smoke_iterations = 0;
    int timeout_ms = 2000;
    std::optional<std::pair<std::uint16_t, std::uint16_t>> vid_pid;
    std::optional<std::pair<std::uint8_t, std::uint8_t>> bus_addr;
};

void usage(const char* argv0) {
    std::cout
        << "Usage: " << argv0 << " [options]\n"
        << "  --list                 list USB devices (no vendor requests)\n"
        << "  --device VID:PID       restrict AOA probe/switch to one VID:PID (hex)\n"
        << "  --bus-address B:A      further restrict by decimal bus/address\n"
        << "  --switch               perform official AOA identification + START\n"
        << "  --smoke N              run N AMB PING/PONG exchanges on an AOA device\n"
        << "  --timeout-ms N         per USB transfer timeout (default 2000)\n"
        << "\nSafety: without --device, probe/switch sends AOA vendor request 51 to\n"
        << "all non-AOA USB devices. Prefer --list then --device VID:PID.\n";
}

bool parse_u16_hex(const std::string& text, std::uint16_t& out) {
    unsigned value = 0;
    const auto* begin = text.data();
    const auto* end = begin + text.size();
    const auto [ptr, ec] = std::from_chars(begin, end, value, 16);
    if (ec != std::errc{} || ptr != end || value > 0xffffU) return false;
    out = static_cast<std::uint16_t>(value);
    return true;
}

bool parse_u8_dec(const std::string& text, std::uint8_t& out) {
    unsigned value = 0;
    const auto* begin = text.data();
    const auto* end = begin + text.size();
    const auto [ptr, ec] = std::from_chars(begin, end, value, 10);
    if (ec != std::errc{} || ptr != end || value > 255U) return false;
    out = static_cast<std::uint8_t>(value);
    return true;
}

template <typename ParseLeft, typename ParseRight, typename L, typename R>
bool parse_pair(const std::string& text, char delimiter, ParseLeft left_parser,
                ParseRight right_parser, L& left, R& right) {
    const auto pos = text.find(delimiter);
    if (pos == std::string::npos || text.find(delimiter, pos + 1) != std::string::npos) {
        return false;
    }
    return left_parser(text.substr(0, pos), left) && right_parser(text.substr(pos + 1), right);
}

bool selected(const amb::aoa::DeviceId& id, const Options& o) {
    if (o.vid_pid && (id.vid != o.vid_pid->first || id.pid != o.vid_pid->second)) return false;
    if (o.bus_addr && (id.bus != o.bus_addr->first || id.address != o.bus_addr->second)) return false;
    return true;
}

std::optional<amb::aoa::DeviceId> accessory_target(const Options& o) {
    amb::aoa::DeviceId id{};
    id.vid = amb::aoa::kGoogleVid;
    if (o.bus_addr) {
        id.bus = o.bus_addr->first;
        id.address = o.bus_addr->second;
    }
    // Do not copy a pre-AOA PID selector: re-enumeration changes VID/PID.
    return id;
}

}  // namespace

int main(int argc, char** argv) {
    Options o{};
    for (int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if (arg == "--list") {
            o.list = true;
        } else if (arg == "--switch") {
            o.do_switch = true;
        } else if (arg == "--device" && i + 1 < argc) {
            std::uint16_t vid = 0, pid = 0;
            if (!parse_pair(argv[++i], ':', parse_u16_hex, parse_u16_hex, vid, pid)) {
                std::cerr << "Invalid --device; expected hex VID:PID, e.g. 0fce:1234\n";
                return 2;
            }
            o.vid_pid = {{vid, pid}};
        } else if (arg == "--bus-address" && i + 1 < argc) {
            std::uint8_t bus = 0, address = 0;
            if (!parse_pair(argv[++i], ':', parse_u8_dec, parse_u8_dec, bus, address)) {
                std::cerr << "Invalid --bus-address; expected decimal BUS:ADDR\n";
                return 2;
            }
            o.bus_addr = {{bus, address}};
        } else if (arg == "--smoke" && i + 1 < argc) {
            unsigned long long n = 0;
            const std::string text = argv[++i];
            const auto [ptr, ec] = std::from_chars(text.data(), text.data() + text.size(), n);
            if (ec != std::errc{} || ptr != text.data() + text.size() || n == 0) {
                std::cerr << "Invalid --smoke iteration count\n";
                return 2;
            }
            o.smoke_iterations = static_cast<std::size_t>(n);
        } else if (arg == "--timeout-ms" && i + 1 < argc) {
            int value = 0;
            const std::string text = argv[++i];
            const auto [ptr, ec] = std::from_chars(text.data(), text.data() + text.size(), value);
            if (ec != std::errc{} || ptr != text.data() + text.size() || value <= 0) {
                std::cerr << "Invalid --timeout-ms\n";
                return 2;
            }
            o.timeout_ms = value;
        } else if (arg == "-h" || arg == "--help") {
            usage(argv[0]);
            return 0;
        } else {
            std::cerr << "Unknown or incomplete argument: " << arg << "\n";
            usage(argv[0]);
            return 2;
        }
    }

    libusb_context* ctx = nullptr;
    int rc = libusb_init(&ctx);
    if (rc < 0) {
        std::cerr << "libusb_init failed: " << libusb_error_name(rc) << "\n";
        return 1;
    }

    const auto ids = amb::aoa::list_devices(ctx);
    if (o.list) {
        for (const auto& id : ids) {
            std::cout << amb::aoa::describe(id);
            if (id.vid == amb::aoa::kGoogleVid && amb::aoa::is_accessory_pid(id.pid)) {
                std::cout << " [AOA accessory]";
            }
            std::cout << "\n";
        }
        if (!o.do_switch && o.smoke_iterations == 0) {
            libusb_exit(ctx);
            return 0;
        }
    }

    if (o.smoke_iterations > 0) {
        int result = 0;
        {
            amb::AccessoryDevice accessory(ctx);
            std::string error;
            if (!accessory.connect(accessory_target(o), error)) {
                std::cerr << "G0.5 connect failed: " << error << "\n";
                result = 6;
            } else {
                std::cout << "[G0.5] connected " << amb::aoa::describe(accessory.id()) << "\n";
                if (!accessory.smoke_test(o.smoke_iterations, o.timeout_ms, error)) {
                    std::cerr << "G0.5 failed: " << error << "\n";
                    result = 7;
                }
            }
        }  // AccessoryDevice must release libusb handles before libusb_exit().
        libusb_exit(ctx);
        return result;
    }

    libusb_device** list = nullptr;
    const auto count = libusb_get_device_list(ctx, &list);
    if (count < 0) {
        std::cerr << "libusb_get_device_list failed: "
                  << libusb_error_name(static_cast<int>(count)) << "\n";
        libusb_exit(ctx);
        return 1;
    }

    std::cout << "AMB G0 AOA probe; ADB is not used.\n";
    libusb_device* candidate = nullptr;
    bool already_accessory = false;

    for (ssize_t i = 0; i < count; ++i) {
        libusb_device_descriptor desc{};
        if (libusb_get_device_descriptor(list[i], &desc) != 0) continue;
        const amb::aoa::DeviceId id{
            .bus = libusb_get_bus_number(list[i]),
            .address = libusb_get_device_address(list[i]),
            .vid = desc.idVendor,
            .pid = desc.idProduct,
        };
        if (!selected(id, o)) continue;

        if (id.vid == amb::aoa::kGoogleVid && amb::aoa::is_accessory_pid(id.pid)) {
            already_accessory = true;
            std::cout << "[AOA] already accessory: " << amb::aoa::describe(id) << "\n";
            continue;
        }

        auto probe = amb::aoa::probe_protocol(list[i]);
        if (probe.protocol && *probe.protocol > 0) {
            std::cout << "[AOA] protocol=" << *probe.protocol
                      << " on " << amb::aoa::describe(probe.id) << "\n";
            if (!candidate) candidate = list[i];
        } else if (!probe.error.empty() && o.vid_pid) {
            std::cout << "[AOA] probe failed on selected device: " << probe.error << "\n";
        }
    }

    if (!o.do_switch) {
        if (already_accessory) {
            std::cout << "G0 already in accessory mode.\n";
        } else if (!candidate) {
            std::cout << "No selected device answered AOA GET_PROTOCOL.\n";
        } else {
            std::cout << "G0 probe passed. Re-run with --switch using the same selector.\n";
        }
        libusb_free_device_list(list, 1);
        libusb_exit(ctx);
        return (candidate || already_accessory) ? 0 : 3;
    }

    if (already_accessory) {
        std::cout << "[PASS G0] already in AOA accessory mode.\n";
        libusb_free_device_list(list, 1);
        libusb_exit(ctx);
        return 0;
    }
    if (!candidate) {
        std::cerr << "No device available for AOA transition.\n";
        libusb_free_device_list(list, 1);
        libusb_exit(ctx);
        return 3;
    }

    const auto candidate_bus = libusb_get_bus_number(candidate);
    std::string error;
    if (!amb::aoa::request_accessory_mode(candidate, error)) {
        std::cerr << "AOA transition request failed: " << error << "\n";
        libusb_free_device_list(list, 1);
        libusb_exit(ctx);
        return 4;
    }

    libusb_free_device_list(list, 1);
    std::cout << "AOA START sent; waiting for USB re-enumeration...\n";
    const auto accessory = amb::aoa::wait_for_accessory(ctx, 8000, candidate_bus);
    if (!accessory) {
        std::cerr << "Timed out waiting for an AOA accessory VID/PID.\n";
        libusb_exit(ctx);
        return 5;
    }

    std::cout << "[PASS G0] " << amb::aoa::describe(*accessory) << "\n";
    if (accessory->pid == amb::aoa::kAccessoryPid) {
        std::cout << "Target accessory-only PID 2d00 observed.\n";
    } else if (accessory->pid == amb::aoa::kAccessoryAdbPid) {
        std::cout << "WARNING: 2d01 includes ADB; target evidence requires USB debugging OFF.\n";
    }

    libusb_exit(ctx);
    return 0;
}
