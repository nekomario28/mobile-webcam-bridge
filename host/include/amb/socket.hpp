#pragma once

#include <string>

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <winsock2.h>
#include <ws2tcpip.h>
#else
#include <sys/socket.h>
#include <unistd.h>
#include <cerrno>
#include <cstring>
#endif

namespace amb::net {

#ifdef _WIN32
using Socket = SOCKET;
using AddressLength = int;
inline constexpr Socket invalid_socket = INVALID_SOCKET;

inline bool initialize(std::string& error) {
    struct Runtime {
        WSADATA data{};
        int result = WSAStartup(MAKEWORD(2, 2), &data);
        ~Runtime() { if (result == 0) WSACleanup(); }
    };
    static Runtime runtime;
    if (runtime.result == 0) return true;
    error = "WSAStartup failed: " + std::to_string(runtime.result);
    return false;
}
inline void close(Socket socket) { closesocket(socket); }
inline int last_error() { return WSAGetLastError(); }
inline std::string error_text(int code) { return "Winsock error " + std::to_string(code); }
inline bool interrupted(int code) { return code == WSAEINTR; }
inline bool would_block(int code) { return code == WSAEWOULDBLOCK; }
inline int set_option(Socket socket, int level, int option, const int& value) {
    return setsockopt(socket, level, option, reinterpret_cast<const char*>(&value), sizeof(value));
}
inline int get_error(Socket socket, int& error) {
    int size = sizeof(error);
    return getsockopt(socket, SOL_SOCKET, SO_ERROR, reinterpret_cast<char*>(&error), &size);
}
inline int send(Socket socket, const void* data, int size) {
    return ::send(socket, static_cast<const char*>(data), size, 0);
}
inline int receive(Socket socket, void* data, int size) {
    return ::recv(socket, static_cast<char*>(data), size, 0);
}
#else
using Socket = int;
using AddressLength = socklen_t;
inline constexpr Socket invalid_socket = -1;
inline bool initialize(std::string&) { return true; }
inline void close(Socket socket) { ::close(socket); }
inline int last_error() { return errno; }
inline std::string error_text(int code) { return std::strerror(code); }
inline bool interrupted(int code) { return code == EINTR; }
inline bool would_block(int code) { return code == EAGAIN || code == EWOULDBLOCK; }
inline int set_option(Socket socket, int level, int option, const int& value) {
    return setsockopt(socket, level, option, &value, sizeof(value));
}
inline int get_error(Socket socket, int& error) {
    socklen_t size = sizeof(error);
    return getsockopt(socket, SOL_SOCKET, SO_ERROR, &error, &size);
}
inline auto send(Socket socket, const void* data, int size) {
    return ::send(socket, data, static_cast<std::size_t>(size), MSG_NOSIGNAL);
}
inline auto receive(Socket socket, void* data, int size) {
    return ::recv(socket, data, static_cast<std::size_t>(size), 0);
}
#endif

}  // namespace amb::net
