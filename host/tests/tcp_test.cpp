#include "amb/tcp.hpp"
#include "amb/wire.hpp"

#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>

#include <array>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <span>
#include <string>
#include <thread>
#include <vector>

namespace {

bool expect(bool condition, const char* message) {
    if (condition) return true;
    std::cerr << "tcp_test: FAIL: " << message << '\n';
    return false;
}

bool read_exact(int fd, std::uint8_t* data, std::size_t size) {
    std::size_t offset = 0;
    while (offset < size) {
        const ssize_t count = ::recv(fd, data + offset, size - offset, 0);
        if (count > 0) {
            offset += static_cast<std::size_t>(count);
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        return false;
    }
    return true;
}

bool write_all(int fd, std::span<const std::uint8_t> bytes) {
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const ssize_t count = ::send(fd, bytes.data() + offset, bytes.size() - offset,
                                     MSG_NOSIGNAL);
        if (count > 0) {
            offset += static_cast<std::size_t>(count);
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        return false;
    }
    return true;
}

bool read_frame(int fd, amb::wire::Header& header, std::vector<std::uint8_t>& payload) {
    std::array<std::uint8_t, amb::wire::kHeaderSize> bytes{};
    if (!read_exact(fd, bytes.data(), bytes.size())) return false;
    std::string error;
    const auto decoded = amb::wire::decode_header(bytes, error);
    if (!decoded) return false;
    header = *decoded;
    payload.assign(header.payload_len, 0);
    return payload.empty() || read_exact(fd, payload.data(), payload.size());
}

bool send_frame(int fd, amb::wire::Type type, std::uint32_t sequence,
                std::span<const std::uint8_t> payload = {}) {
    const auto frame = amb::wire::make_frame(type, 0, sequence, 0, payload);
    return !frame.empty() && write_all(fd, frame);
}

class LoopbackServer {
  public:
    LoopbackServer() {
        fd_ = ::socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (fd_ < 0) return;
        const int one = 1;
        ::setsockopt(fd_, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one));
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        if (::bind(fd_, reinterpret_cast<const sockaddr*>(&address), sizeof(address)) < 0 ||
            ::listen(fd_, 1) < 0) {
            ::close(fd_);
            fd_ = -1;
            return;
        }
        socklen_t length = sizeof(address);
        if (::getsockname(fd_, reinterpret_cast<sockaddr*>(&address), &length) < 0) {
            ::close(fd_);
            fd_ = -1;
            return;
        }
        port_ = ntohs(address.sin_port);
    }

    ~LoopbackServer() {
        if (fd_ >= 0) ::close(fd_);
    }

    [[nodiscard]] bool valid() const { return fd_ >= 0 && port_ != 0; }
    [[nodiscard]] std::uint16_t port() const { return port_; }
    int accept_one() const { return ::accept4(fd_, nullptr, nullptr, SOCK_CLOEXEC); }

  private:
    int fd_ = -1;
    std::uint16_t port_ = 0;
};

bool run_success_case() {
    LoopbackServer server;
    if (!expect(server.valid(), "create loopback server")) return false;
    std::atomic_bool server_ok{true};
    std::thread peer([&] {
        const int fd = server.accept_one();
        if (fd < 0) {
            server_ok = false;
            return;
        }
        amb::wire::Header hello{};
        std::vector<std::uint8_t> payload;
        if (!read_frame(fd, hello, payload) || hello.type != amb::wire::Type::Hello ||
            hello.sequence != 1 || !payload.empty() ||
            !send_frame(fd, amb::wire::Type::HelloAck, hello.sequence)) {
            server_ok = false;
            ::close(fd);
            return;
        }

        amb::wire::Header request{};
        payload.clear();
        if (!read_frame(fd, request, payload) ||
            request.type != amb::wire::Type::VideoIdrRequest || !payload.empty()) {
            server_ok = false;
            ::close(fd);
            return;
        }

        const std::array<std::uint8_t, 8> ping{0, 1, 2, 3, 4, 5, 6, 7};
        if (!send_frame(fd, amb::wire::Type::Ping, 9, ping)) server_ok = false;
        ::close(fd);
    });

    amb::TcpConnection client;
    std::string error;
    bool ok = expect(client.connect("127.0.0.1", server.port(), 2000, error),
                     "empty HELLO handshake") &&
              expect(client.send_frame(amb::wire::Type::VideoIdrRequest, 0, 2, 0, {}, 2000,
                                       error),
                     "send IDR request");
    amb::wire::Header ping{};
    std::vector<std::uint8_t> payload;
    ok &= expect(client.read_frame(ping, payload, 2000, error), "read framed PING");
    ok &= expect(ping.type == amb::wire::Type::Ping && ping.sequence == 9,
                 "PING header round trip");
    ok &= expect(payload == std::vector<std::uint8_t>({0, 1, 2, 3, 4, 5, 6, 7}),
                 "PING payload round trip");
    peer.join();
    ok &= expect(server_ok.load(), "server protocol checks");
    return ok;
}

bool run_invalid_host_case() {
    amb::TcpConnection client;
    std::string error;
    return expect(!client.connect("", 48527, 2000, error),
                  "empty LAN host is rejected locally");
}

}  // namespace

int main() {
    bool ok = run_success_case();
    ok &= run_invalid_host_case();

    if (!ok) return 1;
    std::cout << "tcp_test: PASS\n";
    return 0;
}
