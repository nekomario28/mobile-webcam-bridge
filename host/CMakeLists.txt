cmake_minimum_required(VERSION 3.20)
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/../VERSION" AMB_VERSION LIMIT_COUNT 1)
project(mobile_webcam_host VERSION "${AMB_VERSION}" LANGUAGES CXX)

option(AMB_BUILD_GUI "Build the Qt 6 desktop GUI" OFF)

if(NOT WIN32 AND NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
    message(FATAL_ERROR "The host supports Linux and Windows")
endif()

if(NOT WIN32)
    configure_file(../linux/mobile-webcam.metainfo.xml.in
                   dev.nekomario.mobile-webcam.appdata.xml @ONLY)
endif()

if(MSVC)
    add_compile_options(/W4)
else()
    add_compile_options(-Wall -Wextra -Wpedantic)
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBUSB REQUIRED IMPORTED_TARGET libusb-1.0)
pkg_check_modules(FFMPEG REQUIRED IMPORTED_TARGET libavcodec libavutil libswscale)

add_library(amb-core STATIC
    src/aoa.cpp
    src/accessory.cpp
    src/tcp.cpp
    src/image_transform.cpp
    src/wire.cpp
    src/yuyv.cpp
)
target_include_directories(amb-core PUBLIC include)
target_link_libraries(amb-core PUBLIC PkgConfig::LIBUSB)
if(WIN32)
    target_link_libraries(amb-core PUBLIC ws2_32)
endif()

add_executable(amb-aoa-probe src/main.cpp)
target_link_libraries(amb-aoa-probe PRIVATE amb-core)

add_executable(amb-video-dump src/video_dump_main.cpp)
target_link_libraries(amb-video-dump PRIVATE amb-core)

if(WIN32)
    set(AMB_SINK amb-windows-sink)
else()
    set(AMB_SINK amb-v4l2-sink)
endif()
add_executable(${AMB_SINK} src/video_sink_main.cpp src/hw_decode.cpp)
target_link_libraries(${AMB_SINK} PRIVATE amb-core PkgConfig::FFMPEG)
if(WIN32)
    target_link_libraries(${AMB_SINK} PRIVATE advapi32)
endif()

if(AMB_BUILD_GUI)
    find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets)
    if(WIN32)
        add_executable(mobile-webcam WIN32 src/gui_main.cpp)
    else()
        add_executable(mobile-webcam src/gui_main.cpp)
    endif()
    target_link_libraries(mobile-webcam PRIVATE Qt6::Widgets)
    target_compile_definitions(mobile-webcam PRIVATE AMB_VERSION="${PROJECT_VERSION}")

    install(TARGETS mobile-webcam amb-aoa-probe ${AMB_SINK}
            RUNTIME DESTINATION bin)
    if(NOT WIN32)
        install(FILES ../linux/mobile-webcam.desktop
            DESTINATION share/applications RENAME dev.nekomario.mobile-webcam.desktop)
        install(FILES "${CMAKE_CURRENT_BINARY_DIR}/dev.nekomario.mobile-webcam.appdata.xml"
            DESTINATION share/metainfo)
        install(FILES ../linux/mobile-webcam.svg
            DESTINATION share/icons/hicolor/scalable/apps)
    endif()
    install(FILES ../LICENSE ../THIRD_PARTY_NOTICES.md ../VERSION ../CHANGELOG.md
            DESTINATION share/doc/mobile-webcam)
endif()

include(CTest)
if(BUILD_TESTING)
    add_executable(amb-wire-test tests/wire_test.cpp)
    target_link_libraries(amb-wire-test PRIVATE amb-core)
    add_test(NAME wire COMMAND amb-wire-test)

    add_executable(amb-yuyv-test tests/yuyv_test.cpp)
    target_link_libraries(amb-yuyv-test PRIVATE amb-core)
    add_test(NAME yuyv COMMAND amb-yuyv-test)

    add_executable(amb-image-transform-test tests/image_transform_test.cpp)
    target_link_libraries(amb-image-transform-test PRIVATE amb-core)
    add_test(NAME image-transform COMMAND amb-image-transform-test)

    add_executable(amb-tcp-test tests/tcp_test.cpp)
    target_link_libraries(amb-tcp-test PRIVATE amb-core)
    add_test(NAME tcp COMMAND amb-tcp-test)
    set_tests_properties(tcp PROPERTIES TIMEOUT 15)
    if(WIN32)
        add_executable(amb-unity-capture-test tests/unity_capture_test.cpp)
        add_test(NAME unity-capture COMMAND amb-unity-capture-test)
        set_tests_properties(unity-capture PROPERTIES TIMEOUT 15)
    endif()
endif()
