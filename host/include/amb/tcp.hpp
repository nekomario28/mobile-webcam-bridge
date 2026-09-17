#pragma once

#include "amb/wire.hpp"

#include <cstdint>
#include <span>
#include <string>
#include <vector>

namespace amb {

class TcpConnection {
  public:
    TcpConnection() = default;
    ~TcpConnection();

    TcpConnection(const TcpConnection&) = delete;
    TcpConnection& operator=(const TcpConnection&) = delete;

    bool connect(const std::string& host, std::uint16_t port, int timeout_ms,
                 std::string& error);
    void disconnect();
    [[nodiscard]] bool connected() const { return fd_ >= 0; }

    bool send_frame(wire::Type type, std::uint16_t flags, std::uint32_t sequence,
                    std::uint64_t pts_us, std::span<const std::uint8_t> payload,
                    int timeout_ms, std::string& error);
    bool read_frame(wire::Header& header, std::vector<std::uint8_t>& payload,
                    int timeout_ms, std::string& error);

  private:
    bool write_all(const std::uint8_t* data, std::size_t size, int timeout_ms,
                   std::string& error);
    bool read_exact(std::uint8_t* data, std::size_t size, int timeout_ms,
                    std::string& error);

    int fd_ = -1;
};

}  // namespace amb
