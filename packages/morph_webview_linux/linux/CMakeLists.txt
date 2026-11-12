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

target_include_directories(${PLUGIN_NAME} INTERFACE
  "${CMAKE_CURRENT_SOURCE_DIR}/include")
target_link_libraries(${PLUGIN_NAME} PRIVATE flutter)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::GTK)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::WEBKIT)
target_link_libraries(${PLUGIN_NAME} PRIVATE PkgConfig::EGL)

set(morph_webview_linux_bundled_libraries
  ""
  PARENT_SCOPE
)
