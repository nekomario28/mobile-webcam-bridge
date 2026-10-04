#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <string>
#include <thread>

int main(int argc, char** argv) {
    const char* state = std::getenv("AMB_GUI_FIXTURE_STATE");
    if (!state) return 2;
    if (argc > 1 && std::string(argv[1]) == "--list") {
        FILE* file = std::fopen(state, "rb");
        const bool switched = file != nullptr;
        if (file) std::fclose(file);
        std::cout << (switched
            ? "bus=1 addr=3 VID=18d1 PID=2d00 [AOA accessory]\n"
            : "bus=1 addr=2 VID=0fce PID=020d\n");
        return 0;
    }
    for (int i = 1; i < argc; ++i) {
        if (std::string(argv[i]) == "--switch") {
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
            FILE* file = std::fopen(state, "wb");
            if (!file) return 3;
            std::fputc('1', file);
            std::fclose(file);
            std::cout << "[PASS G0] bus=1 addr=3 VID=18d1 PID=2d00\n";
            return 0;
        }
    }
    FILE* arguments = std::fopen((std::string(state) + ".args").c_str(), "w");
    if (!arguments) return 3;
    for (int i = 1; i < argc; ++i) std::fprintf(arguments, "%s\n", argv[i]);
    std::fclose(arguments);
    std::string command;
    while (std::getline(std::cin, command)) {
        std::cout << "transform rotation=0 horizontal_flip=on vertical_flip=off\n" << std::flush;
    }
    return 0;
}
