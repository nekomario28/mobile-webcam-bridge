#include "amb/tcp.hpp"

#include <arpa/inet.h>
#include <fcntl.h>
#include <netdb.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <sys/socket.h>
#include <unistd.h>

#include <array>
#include <algorithm>
#include <cerrno>
#include <cctype>
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

bool wait_fd(int fd, short events, std::chrono::steady_clock::time_point deadline,
             std::string& error) {
    while (true) {
        const int timeout = remaining_ms(deadline);
        if (timeout == 0) {
            error = "TCP operation timed out";
            return false;
        }
        pollfd pfd{.fd = fd, .events = events, .revents = 0};
        const int rc = ::poll(&pfd, 1, timeout);
        if (rc > 0) {
            if ((pfd.revents & (POLLERR | POLLHUP | POLLNVAL)) != 0) {
                int socket_error = 0;
                socklen_t length = sizeof(socket_error);
                ::getsockopt(fd, SOL_SOCKET, SO_ERROR, &socket_error, &length);
                error = socket_error == 0 ? "TCP connection closed" : std::strerror(socket_error);
                return false;
            }
            if ((pfd.revents & events) != 0) return true;
            continue;
        }
        if (rc == 0) {
            error = "TCP operation timed out";
            return false;
        }
        if (errno == EINTR) continue;
        error = std::strerror(errno);
        return false;
    }
}

}  // namespace

TcpConnection::~TcpConnection() { disconnect(); }

bool TcpConnection::connect(const std::string& host, std::uint16_t port,
                            const std::string& pin, int timeout_ms, std::string& error) {
    disconnect();
    const bool numeric_pin = pin.size() == 6 &&
        std::all_of(pin.begin(), pin.end(), [](unsigned char ch) { return std::isdigit(ch) != 0; });
    if (host.empty() || !numeric_pin) {
        error = "LAN host and six-digit PIN are required";
        return false;
    }

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
        const int candidate = ::socket(address->ai_family, address->ai_socktype | SOCK_CLOEXEC,
                                       address->ai_protocol);
        if (candidate < 0) continue;
        const int old_flags = ::fcntl(candidate, F_GETFL, 0);
        if (old_flags < 0 || ::fcntl(candidate, F_SETFL, old_flags | O_NONBLOCK) < 0) {
            ::close(candidate);
            continue;
        }
        int rc = ::connect(candidate, address->ai_addr, address->ai_addrlen);
        if (rc < 0 && errno != EINPROGRESS) {
            ::close(candidate);
            continue;
        }
        if (rc < 0) {
            std::string wait_error;
            if (!wait_fd(candidate, POLLOUT, deadline, wait_error)) {
                ::close(candidate);
                error = wait_error;
                continue;
            }
            int socket_error = 0;
            socklen_t length = sizeof(socket_error);
            if (::getsockopt(candidate, SOL_SOCKET, SO_ERROR, &socket_error, &length) < 0 ||
                socket_error != 0) {
                error = socket_error == 0 ? std::strerror(errno) : std::strerror(socket_error);
                ::close(candidate);
                continue;
            }
        }
        ::fcntl(candidate, F_SETFL, old_flags);
        const int one = 1;
        ::setsockopt(candidate, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));
        fd_ = candidate;
        break;
    }
    if (fd_ < 0) {
        if (error.empty()) error = "unable to connect to LAN endpoint";
        return false;
    }

    const std::vector<std::uint8_t> pin_payload(pin.begin(), pin.end());
    if (!send_frame(wire::Type::Hello, 0, 1, 0, pin_payload, timeout_ms, error)) {
        disconnect();
        return false;
    }
    wire::Header ack{};
    std::vector<std::uint8_t> ack_payload;
    if (!read_frame(ack, ack_payload, timeout_ms, error) ||
        ack.type != wire::Type::HelloAck || ack.sequence != 1 || !ack_payload.empty()) {
        if (error.empty()) error = "LAN pairing handshake failed";
        disconnect();
        return false;
    }
    return true;
}

void TcpConnection::disconnect() {
    if (fd_ >= 0) ::close(fd_);
    fd_ = -1;
}

bool TcpConnection::write_all(const std::uint8_t* data, std::size_t size, int timeout_ms,
                              std::string& error) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::size_t offset = 0;
    while (offset < size) {
        if (!wait_fd(fd_, POLLOUT, deadline, error)) return false;
        const ssize_t n = ::send(fd_, data + offset, size - offset, MSG_NOSIGNAL);
        if (n > 0) {
            offset += static_cast<std::size_t>(n);
            continue;
        }
        if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
        error = n == 0 ? "TCP write closed" : std::strerror(errno);
        return false;
    }
    return true;
}

bool TcpConnection::read_exact(std::uint8_t* data, std::size_t size, int timeout_ms,
                               std::string& error) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
    std::size_t offset = 0;
    while (offset < size) {
        if (!wait_fd(fd_, POLLIN, deadline, error)) return false;
        const ssize_t n = ::recv(fd_, data + offset, size - offset, 0);
        if (n > 0) {
            offset += static_cast<std::size_t>(n);
            continue;
        }
        if (n < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
        error = n == 0 ? "TCP peer closed" : std::strerror(errno);
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
