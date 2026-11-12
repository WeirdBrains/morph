# The Flutter tooling requires that developers have CMake 3.10 or later
# installed. You should not increase this version, as doing so will cause
# the plugin to fail to compile for some customers of the plugin.
cmake_minimum_required(VERSION 3.10)

set(PROJECT_NAME "morph_webview_linux")
project(${PROJECT_NAME} LANGUAGES CXX)

set(PLUGIN_NAME "morph_webview_linux_plugin")

find_package(PkgConfig REQUIRED)
pkg_check_modules(WEBKIT REQUIRED IMPORTED_TARGET webkit2gtk-4.1)
pkg_check_modules(EGL REQUIRED IMPORTED_TARGET egl)

# Optional WPE path (WPE Platform or WPEBackend-fdo). When headers are present
# the DMA-BUF backend is compiled in; otherwise feature detection reports
# wpe-not-built and the snapshot fallback is used.
pkg_check_modules(WPE QUIET IMPORTED_TARGET wpe-webkit-2.0)
if(NOT WPE_FOUND)
  pkg_check_modules(WPE QUIET IMPORTED_TARGET wpe-webkit-1.1)
endif()
pkg_check_modules(WPE_FDO QUIET IMPORTED_TARGET wpebackend-fdo-1.0)
pkg_check_modules(WPE_PLATFORM QUIET IMPORTED_TARGET wpe-platform-2.0)

list(APPEND PLUGIN_SOURCES
  "morph_webview_linux_plugin.cc"
  "render_path.cc"
)

add_library(${PLUGIN_NAME} SHARED
  ${PLUGIN_SOURCES}
)

apply_standard_settings(${PLUGIN_NAME})

set_target_properties(${PLUGIN_NAME} PROPERTIES
  CXX_VISIBILITY_PRESET hidden)
target_compile_definitions(${PLUGIN_NAME} PRIVATE FLUTTER_PLUGIN_IMPL)

if(WPE_FOUND AND (WPE_FDO_FOUND OR WPE_PLATFORM_FOUND))
  target_compile_definitions(${PLUGIN_NAME} PRIVATE MORPH_HAS_WPE=1)
  target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::WPE)
  if(WPE_FDO_FOUND)
    target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::WPE_FDO)
  endif()
  if(WPE_PLATFORM_FOUND)
    target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::WPE_PLATFORM)
  endif()
  message(STATUS "morph_webview_linux: building WPE DMA-BUF path")
else()
  message(STATUS "morph_webview_linux: WPE not found; snapshot path only")
endif()

# In-process gRPC control layer (staticlib). Never drop cargo target/ inside the repo.
if(DEFINED ENV{CARGO_TARGET_DIR} AND NOT "$ENV{CARGO_TARGET_DIR}" STREQUAL "")
  set(MORPH_CARGO_TARGET_DIR "$ENV{CARGO_TARGET_DIR}")
else()
  set(MORPH_CARGO_TARGET_DIR "/tmp/morph-cargo-target")
endif()
set(MORPH_CONTROL_LIB "${MORPH_CARGO_TARGET_DIR}/release/libmorph_webview_control.a")
# Flutter adds this file via an ephemeral symlink; resolve it so paths stay in the repo.
get_filename_component(MORPH_PLUGIN_LINUX_DIR "${CMAKE_CURRENT_LIST_DIR}" REALPATH)
get_filename_component(MORPH_WORKSPACE_DIR "${MORPH_PLUGIN_LINUX_DIR}/../../.." REALPATH)

add_custom_command(
  OUTPUT "${MORPH_CONTROL_LIB}"
  COMMAND ${CMAKE_COMMAND} -E env
    "CARGO_TARGET_DIR=${MORPH_CARGO_TARGET_DIR}"
    cargo build --release --locked -p morph-webview-control
  WORKING_DIRECTORY "${MORPH_WORKSPACE_DIR}"
  DEPENDS
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/src/lib.rs"
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/src/server.rs"
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/src/ffi.rs"
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/src/endpoint.rs"
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/Cargo.toml"
    "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/proto/morph.webview.v1.proto"
  COMMENT "Building morph-webview-control staticlib"
  VERBATIM
)
add_custom_target(morph_webview_control_lib DEPENDS "${MORPH_CONTROL_LIB}")
add_dependencies(${PLUGIN_NAME} morph_webview_control_lib)

target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_include_directories(${PLUGIN_NAME} PRIVATE
  "${MORPH_WORKSPACE_DIR}/crates/morph-webview-control/include")
target_link_libraries(${PLUGIN_NAME} PRIVATE flutter)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::GTK)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::WEBKIT)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::EGL)
target_link_libraries(${PLUGIN_NAME} PRIVATE "${MORPH_CONTROL_LIB}" pthread dl m)

set(morph_webview_linux_bundled_libraries
  ""
  PARENT_SCOPE
)
