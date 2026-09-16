cmake_minimum_required(VERSION 3.20)
file(STRINGS "${CMAKE_CURRENT_SOURCE_DIR}/../VERSION" AMB_VERSION LIMIT_COUNT 1)
project(mobile_webcam_host VERSION "${AMB_VERSION}" LANGUAGES CXX)

option(AMB_BUILD_GUI "Build the Qt 6 desktop GUI" OFF)

configure_file(../linux/mobile-webcam.metainfo.xml.in
               dev.nekomario.mobile-webcam.appdata.xml @ONLY)

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
    src/hw_decode.cpp
    src/image_transform.cpp
    src/wire.cpp
    src/yuyv.cpp
)
target_include_directories(amb-core PUBLIC include)
target_link_libraries(amb-core PUBLIC PkgConfig::LIBUSB)
target_compile_options(amb-core PRIVATE -Wall -Wextra -Wpedantic)

add_executable(amb-aoa-probe src/main.cpp)
target_link_libraries(amb-aoa-probe PRIVATE amb-core)
target_compile_options(amb-aoa-probe PRIVATE -Wall -Wextra -Wpedantic)

add_executable(amb-video-dump src/video_dump_main.cpp)
target_link_libraries(amb-video-dump PRIVATE amb-core)
target_compile_options(amb-video-dump PRIVATE -Wall -Wextra -Wpedantic)

add_executable(amb-v4l2-sink src/v4l2_sink_main.cpp)
target_link_libraries(amb-v4l2-sink PRIVATE amb-core PkgConfig::FFMPEG)
target_compile_options(amb-v4l2-sink PRIVATE -Wall -Wextra -Wpedantic)

if(AMB_BUILD_GUI)
    find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets)
    add_executable(mobile-webcam src/gui_main.cpp)
    target_link_libraries(mobile-webcam PRIVATE Qt6::Widgets)
    target_compile_options(mobile-webcam PRIVATE -Wall -Wextra -Wpedantic)
    target_compile_definitions(mobile-webcam PRIVATE AMB_VERSION="${PROJECT_VERSION}")

    install(TARGETS mobile-webcam amb-aoa-probe amb-v4l2-sink
            RUNTIME DESTINATION bin)
    install(FILES ../linux/mobile-webcam.desktop
            DESTINATION share/applications RENAME dev.nekomario.mobile-webcam.desktop)
    install(FILES "${CMAKE_CURRENT_BINARY_DIR}/dev.nekomario.mobile-webcam.appdata.xml"
            DESTINATION share/metainfo)
    install(FILES ../linux/mobile-webcam.svg
            DESTINATION share/icons/hicolor/scalable/apps)
    install(FILES ../LICENSE ../THIRD_PARTY_NOTICES.md ../VERSION ../CHANGELOG.md
            DESTINATION share/doc/mobile-webcam)
endif()

include(CTest)
if(BUILD_TESTING)
    add_executable(amb-wire-test tests/wire_test.cpp)
    target_link_libraries(amb-wire-test PRIVATE amb-core)
    target_compile_options(amb-wire-test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME wire COMMAND amb-wire-test)

    add_executable(amb-yuyv-test tests/yuyv_test.cpp)
    target_link_libraries(amb-yuyv-test PRIVATE amb-core)
    target_compile_options(amb-yuyv-test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME yuyv COMMAND amb-yuyv-test)

    add_executable(amb-image-transform-test tests/image_transform_test.cpp)
    target_link_libraries(amb-image-transform-test PRIVATE amb-core)
    target_compile_options(amb-image-transform-test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME image-transform COMMAND amb-image-transform-test)

    add_executable(amb-tcp-test tests/tcp_test.cpp)
    target_link_libraries(amb-tcp-test PRIVATE amb-core)
    target_compile_options(amb-tcp-test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME tcp COMMAND amb-tcp-test)
endif()
