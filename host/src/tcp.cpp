#include "amb/tcp.hpp"

#ifndef _WIN32
#include <fcntl.h>
#include <netdb.h>
#include <netinet/tcp.h>
#include <poll.h>
#endif

#include <array>
#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstring>
#include <limits>
#include <memory>

namespace amb {
namespace {

int remaining_ms(std::chrono::steady_clock::time_point deadline) {
    const auto now = std::chrono::steady_clock::now();
    if (now >= deadline) return 0;
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - now).count();
    return static_cast<int>(std::min<long long>(std::max<long long>(1, ms),
                                                std::numeric_limits<int>::max()));
}

bool wait_fd(net::Socket fd, bool writing, std::chrono::steady_clock::time_point deadline,
             std::string& error) {
    while (true) {
        const int timeout = remaining_ms(deadline);
        if (timeout == 0) {
            error = "TCP operation timed out";
            return false;
        }
#ifdef _WIN32
        fd_set ready, failed;
        FD_ZERO(&ready);
        FD_ZERO(&failed);
        FD_SET(fd, &ready);
        FD_SET(fd, &failed);
        timeval duration{};
        duration.tv_sec = timeout / 1000;
        duration.tv_usec = (timeout % 1000) * 1000;
        const int rc = ::select(0, writing ? nullptr : &ready,
                                writing ? &ready : nullptr, &failed, &duration);
        if (rc > 0) {
            if (FD_ISSET(fd, &failed)) {
                int code = 0;
                if (net::get_error(fd, code) != 0) code = net::last_error();
                error = net::error_text(code);
                return false;
            }
            return true;
        }
#else
        pollfd descriptor{.fd = fd, .events = static_cast<short>(writing ? POLLOUT : POLLIN),
                           .revents = 0};
        const int rc = ::poll(&descriptor, 1, timeout);
        if (rc > 0) {
            // Drain queued data even when the peer has already closed its socket.
            if ((descriptor.revents & descriptor.events) != 0) return true;
            int code = 0;
            if (net::get_error(fd, code) != 0) code = net::last_error();
            error = code == 0 ? "TCP connection closed" : net::error_text(code);
            return false;
        }
#endif
        if (rc == 0) {
            error = "TCP operation timed out";
            return false;
        }
        const int code = net::last_error();
        if (net::interrupted(code)) continue;
        error = net::error_text(code);
        return false;
    }
}

}  // namespace

TcpConnection::~TcpConnection() { disconnect(); }

bool TcpConnection::connect(const std::string& host, std::uint16_t port,
                            int timeout_ms, std::string& error) {
    disconnect();
    if (host.empty()) {
        error = "LAN host is required";
        return false;
    }

    if (!net::initialize(error)) return false;

    addrinfo hints{};
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_protocol = IPPROTO_TCP;
    addrinfo* raw = nullptr;
    const std::string service = std::to_string(port);
    const int gai = ::getaddrinfo(host.c_str(), service.c_str(), &hints, &raw);
    if (gai != 0) {
        error = std::string("resolve ") + host + ": " + gai_strerror(gai);
        return false;
    }
    std::unique_ptr<addrinfo, decltype(&freeaddrinfo)> addresses(raw, freeaddrinfo);
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);

    for (addrinfo* address = raw; address != nullptr; address = address->ai_next) {
#ifdef _WIN32
        const net::Socket candidate = ::socket(address->ai_family, address->ai_socktype,
                                                address->ai_protocol);
        if (candidate == net::invalid_socket) continue;
        u_long nonblocking = 1;
        if (!SetHandleInformation(reinterpret_cast<HANDLE>(candidate), HANDLE_FLAG_INHERIT, 0) ||
            ioctlsocket(candidate, FIONBIO, &nonblocking) != 0) {
            net::close(candidate);
            continue;
        }
#else
        const net::Socket candidate = ::socket(address->ai_family,
                                                address->ai_socktype | SOCK_CLOEXEC,
                                                address->ai_protocol);
        if (candidate == net::invalid_socket) continue;
        const int old_flags = ::fcntl(candidate, F_GETFL, 0);
        if (old_flags < 0 || ::fcntl(candidate, F_SETFL, old_flags | O_NONBLOCK) < 0) {
            net::close(candidate);
            continue;
        }
#endif
        int rc = ::connect(candidate, address->ai_addr, static_cast<net::AddressLength>(address->ai_addrlen));
#ifdef _WIN32
        const bool pending = rc < 0 && net::would_block(net::last_error());
#else
        const bool pending = rc < 0 && errno == EINPROGRESS;
#endif
        if (rc < 0 && !pending) {
            net::close(candidate);
            continue;
        }
        if (rc < 0) {
            std::string wait_error;
            if (!wait_fd(candidate, true, deadline, wait_error)) {
                net::close(candidate);
                error = wait_error;
                continue;
            }
            int socket_error = 0;
            if (net::get_error(candidate, socket_error) < 0 ||
                socket_error != 0) {
                error = net::error_text(socket_error == 0 ? net::last_error() : socket_error);
                net::close(candidate);
                continue;
            }
        }
        const int one = 1;
        net::set_option(candidate, IPPROTO_TCP, TCP_NODELAY, one);
        fd_ = candidate;
        break;
    }
    if (!connected()) {
        if (error.empty()) error = "unable to connect to LAN endpoint";
        return false;
    }

    if (!send_frame(wire::Type::Hello, 0, 1, 0, {}, timeout_ms, error)) {
        disconnect();
        return false;
    }
    wire::Header ack{};
    std::vector<std::uint8_t> ack_payload;
    if (!read_frame(ack, ack_payload, timeout_ms, error) ||
        ack.type != wire::Type::HelloAck || ack.sequence != 1 || !ack_payload.empty()) {
        if (error.empty()) error = "LAN handshake failed";
        disconnect();
        return false;
    }
    return true;
}

void TcpConnection::disconnect() {
    if (connected()) net::close(fd_);
    fd_ = net::invalid_socket;
}

bool TcpConnection::write_all(const std::uint8_t* data, std::size_t size, int timeout_ms,
                              std::string& error) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::size_t offset = 0;
    while (offset < size) {
        if (!wait_fd(fd_, true, deadline, error)) return false;
        const auto length = static_cast<int>(std::min<std::size_t>(size - offset,
                                                                    std::numeric_limits<int>::max()));
        const auto n = net::send(fd_, data + offset, length);
        if (n > 0) {
            offset += static_cast<std::size_t>(n);
            continue;
        }
        const int code = net::last_error();
        if (n < 0 && (net::interrupted(code) || net::would_block(code))) continue;
        error = n == 0 ? "TCP write closed" : net::error_text(code);
        return false;
    }
    return true;
}

bool TcpConnection::read_exact(std::uint8_t* data, std::size_t size, int timeout_ms,
                               std::string& error) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::size_t offset = 0;
    while (offset < size) {
        if (!wait_fd(fd_, false, deadline, error)) return false;
        const auto length = static_cast<int>(std::min<std::size_t>(size - offset,
                                                                    std::numeric_limits<int>::max()));
        const auto n = net::receive(fd_, data + offset, length);
        if (n > 0) {
            offset += static_cast<std::size_t>(n);
            continue;
        }
        const int code = net::last_error();
        if (n < 0 && (net::interrupted(code) || net::would_block(code))) continue;
        error = n == 0 ? "TCP peer closed" : net::error_text(code);
        return false;
    }
    return true;
}

bool TcpConnection::send_frame(wire::Type type, std::uint16_t flags,
                               std::uint32_t sequence, std::uint64_t pts_us,
                               std::span<const std::uint8_t> payload, int timeout_ms,
                               std::string& error) {
    if (!connected()) {
        error = "LAN connection is not open";
        return false;
    }
    const auto frame = wire::make_frame(type, flags, sequence, pts_us, payload);
    if (frame.empty() && !payload.empty()) {
        error = "failed to construct wire frame";
        return false;
    }
    return write_all(frame.data(), frame.size(), timeout_ms, error);
}

bool TcpConnection::read_frame(wire::Header& header, std::vector<std::uint8_t>& payload,
                               int timeout_ms, std::string& error) {
    if (!connected()) {
        error = "LAN connection is not open";
        return false;
    }
    std::array<std::uint8_t, wire::kHeaderSize> bytes{};
    if (!read_exact(bytes.data(), bytes.size(), timeout_ms, error)) return false;
    const auto decoded = wire::decode_header(bytes, error);
    if (!decoded) return false;
    header = *decoded;
    payload.assign(header.payload_len, 0);
    if (!payload.empty() && !read_exact(payload.data(), payload.size(), timeout_ms, error)) {
        payload.clear();
        return false;
    }
    return true;
}

}  // namespace amb
