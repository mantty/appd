cmake_minimum_required(VERSION 4.0)

find_package(cmake-npm REQUIRED PATHS node_modules/cmake-npm ../node_modules/cmake-npm)
find_package(cmake-bare REQUIRED PATHS node_modules/cmake-bare ../node_modules/cmake-bare)
find_package(cmake-fetch REQUIRED PATHS node_modules/cmake-fetch ../node_modules/cmake-fetch)

project(appd_bare LANGUAGES C CXX)

set(APPD_BARE_KIT_TAG "v2.3.0")
set(APPD_BARE_KIT_COMMIT "264ebb068d23ca5c5168ed92ea1a0ce85b12259d")

if(NOT DEFINED APPD_BARE_MODULES_ROOT)
  message(FATAL_ERROR "APPD_BARE_MODULES_ROOT must point to the deployed Bare runtime modules")
endif()

if(APPLE)
  set(
    BARE_ENGINE
    "github:holepunchto/libjsc#df04ed716bf0f8d5b9d4d4e6bccdacd1306f19a7"
    CACHE STRING "Bare JavaScript engine"
    FORCE
  )
endif()

if(ANDROID)
  add_compile_options($<$<CONFIG:MinSizeRel>:-g0>)
endif()

fetch_package("github:holepunchto/bare-kit#${APPD_BARE_KIT_COMMIT}")

if(WIN32)
  # Bare's bundled C++ runtime uses the release CRT for every non-debug build.
  set_property(
    TARGET c++
    PROPERTY INTERFACE_LINK_OPTIONS
      "/DEFAULTLIB:libcpmt$<$<CONFIG:Debug>:d>"
      "/NODEFAULTLIB:libcmt$<$<NOT:$<CONFIG:Debug>>:d>"
  )
endif()

set_target_properties(
  bare_kit
  PROPERTIES
  LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/runtime"
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/runtime"
  ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/runtime"
)

if(WIN32)
  link_bare_modules(
    bare_kit
    EXCLUDE bare-posix
    WORKING_DIRECTORY "${APPD_BARE_MODULES_ROOT}"
  )
else()
  link_bare_modules(bare_kit WORKING_DIRECTORY "${APPD_BARE_MODULES_ROOT}")
endif()

if(ANDROID)
  add_custom_target(
    appd_bare_kit
    COMMAND "${CMAKE_STRIP}" --strip-unneeded "$<TARGET_FILE:bare_kit>"
    DEPENDS bare_kit
  )
else()
  add_custom_target(appd_bare_kit DEPENDS bare_kit)
endif()
