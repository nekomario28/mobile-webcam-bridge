#include "amb/tcp.hpp"
#include "amb/wire.hpp"

#ifndef _WIN32
#include <arpa/inet.h>
#endif

#include <array>
#include <atomic>
#include <cerrno>
#include <chrono>
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

bool read_exact(amb::net::Socket fd, std::uint8_t* data, std::size_t size) {
    std::size_t offset = 0;
    while (offset < size) {
        const auto count = amb::net::receive(fd, data + offset, static_cast<int>(size - offset));
        if (count > 0) {
            offset += static_cast<std::size_t>(count);
            continue;
        }
        if (count < 0 && amb::net::interrupted(amb::net::last_error())) continue;
        return false;
    }
    return true;
}

bool write_all(amb::net::Socket fd, std::span<const std::uint8_t> bytes) {
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto count = amb::net::send(fd, bytes.data() + offset,
                                          static_cast<int>(bytes.size() - offset));
        if (count > 0) {
            offset += static_cast<std::size_t>(count);
            continue;
        }
        if (count < 0 && amb::net::interrupted(amb::net::last_error())) continue;
        return false;
    }
    return true;
}

bool read_frame(amb::net::Socket fd, amb::wire::Header& header, std::vector<std::uint8_t>& payload) {
    std::array<std::uint8_t, amb::wire::kHeaderSize> bytes{};
    if (!read_exact(fd, bytes.data(), bytes.size())) return false;
    std::string error;
    const auto decoded = amb::wire::decode_header(bytes, error);
    if (!decoded) return false;
    header = *decoded;
    payload.assign(header.payload_len, 0);
    return payload.empty() || read_exact(fd, payload.data(), payload.size());
}

bool send_frame(amb::net::Socket fd, amb::wire::Type type, std::uint32_t sequence,
                std::span<const std::uint8_t> payload = {}) {
    const auto frame = amb::wire::make_frame(type, 0, sequence, 0, payload);
    return !frame.empty() && write_all(fd, frame);
}

class LoopbackServer {
  public:
    LoopbackServer() {
        fd_ = ::socket(AF_INET, SOCK_STREAM, 0);
        if (fd_ == amb::net::invalid_socket) return;
        const int one = 1;
        amb::net::set_option(fd_, SOL_SOCKET, SO_REUSEADDR, one);
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        address.sin_port = 0;
        if (::bind(fd_, reinterpret_cast<const sockaddr*>(&address), sizeof(address)) < 0 ||
            ::listen(fd_, 1) < 0) {
            amb::net::close(fd_);
            fd_ = amb::net::invalid_socket;
            return;
        }
        amb::net::AddressLength length = sizeof(address);
        if (::getsockname(fd_, reinterpret_cast<sockaddr*>(&address), &length) < 0) {
            amb::net::close(fd_);
            fd_ = amb::net::invalid_socket;
            return;
        }
        port_ = ntohs(address.sin_port);
    }

    ~LoopbackServer() {
        if (fd_ != amb::net::invalid_socket) amb::net::close(fd_);
    }

    [[nodiscard]] bool valid() const { return fd_ != amb::net::invalid_socket && port_ != 0; }
    [[nodiscard]] std::uint16_t port() const { return port_; }
    amb::net::Socket accept_one() const { return ::accept(fd_, nullptr, nullptr); }

  private:
    amb::net::Socket fd_ = amb::net::invalid_socket;
    std::uint16_t port_ = 0;
};

bool run_success_case() {
    LoopbackServer server;
    if (!expect(server.valid(), "create loopback server")) return false;
    std::atomic_bool server_ok{true};
    std::thread peer([&] {
        const auto fd = server.accept_one();
        if (fd == amb::net::invalid_socket) {
            server_ok = false;
            return;
        }
        amb::wire::Header hello{};
        std::vector<std::uint8_t> payload;
        if (!read_frame(fd, hello, payload) || hello.type != amb::wire::Type::Hello ||
            hello.sequence != 1 || !payload.empty() ||
            !send_frame(fd, amb::wire::Type::HelloAck, hello.sequence)) {
            server_ok = false;
            amb::net::close(fd);
            return;
        }

        amb::wire::Header request{};
        payload.clear();
        if (!read_frame(fd, request, payload) ||
            request.type != amb::wire::Type::VideoIdrRequest || !payload.empty()) {
            server_ok = false;
            amb::net::close(fd);
            return;
        }

        const std::array<std::uint8_t, 8> ping{0, 1, 2, 3, 4, 5, 6, 7};
        if (!send_frame(fd, amb::wire::Type::Ping, 9, ping)) server_ok = false;
        amb::net::close(fd);
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

bool run_timeout_case() {
    LoopbackServer server;
    if (!expect(server.valid(), "timeout server")) return false;
    std::atomic_bool server_ok{true};
    std::thread peer([&] {
        const auto fd = server.accept_one();
        if (fd == amb::net::invalid_socket) { server_ok = false; return; }
        amb::wire::Header hello{};
        std::vector<std::uint8_t> payload;
        if (!read_frame(fd, hello, payload) ||
            !send_frame(fd, amb::wire::Type::HelloAck, 1)) server_ok = false;
        // Keep the socket open while the client times out waiting for a frame.
        std::this_thread::sleep_for(std::chrono::milliseconds(250));
        amb::net::close(fd);
    });
    amb::TcpConnection client;
    std::string error;
    bool ok = expect(client.connect("127.0.0.1", server.port(), 2000, error),
                     "timeout case handshake");
    amb::wire::Header header{};
    std::vector<std::uint8_t> payload;
    const auto before = std::chrono::steady_clock::now();
    ok &= expect(!client.read_frame(header, payload, 50, error) &&
                 error == "TCP operation timed out", "idle read times out");
    ok &= expect(std::chrono::steady_clock::now() - before < std::chrono::seconds(1),
                 "idle read remains bounded");
    client.disconnect();
    peer.join();
    return ok && expect(server_ok, "timeout server protocol");
}

bool run_fragmented_case() {
    LoopbackServer server;
    if (!expect(server.valid(), "fragmentation server")) return false;
    std::atomic_bool server_ok{true};
    std::thread peer([&] {
        const auto fd = server.accept_one();
        if (fd == amb::net::invalid_socket) { server_ok = false; return; }
        amb::wire::Header hello{};
        std::vector<std::uint8_t> payload;
        if (!read_frame(fd, hello, payload)) server_ok = false;
        const auto ack = amb::wire::make_frame(amb::wire::Type::HelloAck, 0, 1, 0, {});
        for (const auto byte : ack) {
            const std::array<std::uint8_t, 1> fragment{byte};
            if (!write_all(fd, fragment)) { server_ok = false; break; }
        }
        // Send half a header and close. A partial frame must never be accepted.
        write_all(fd, std::span<const std::uint8_t>(ack).first(ack.size() / 2));
        amb::net::close(fd);
    });
    amb::TcpConnection client;
    std::string error;
    bool ok = expect(client.connect("127.0.0.1", server.port(), 2000, error),
                     "fragmented handshake");
    amb::wire::Header header{};
    std::vector<std::uint8_t> payload;
    ok &= expect(!client.read_frame(header, payload, 2000, error), "partial frame is rejected");
    peer.join();
    return ok && expect(server_ok, "fragmentation server protocol");
}

}  // namespace

int main() {
    std::string error;
    if (!amb::net::initialize(error)) {
        std::cerr << error << "\n";
        return 1;
    }
    bool ok = run_success_case();
    ok &= run_invalid_host_case();
    ok &= run_timeout_case();
    ok &= run_fragmented_case();

    if (!ok) return 1;
    std::cout << "tcp_test: PASS\n";
    return 0;
}
