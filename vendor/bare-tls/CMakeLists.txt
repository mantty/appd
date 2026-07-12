cmake_minimum_required(VERSION 4.0)

find_package(cmake-bare REQUIRED PATHS node_modules/cmake-bare)
find_package(cmake-fetch REQUIRED PATHS node_modules/cmake-fetch)

project(bare_tls C CXX)

bare_target(target)

if(target MATCHES "win32")
  add_definitions(-DWIN32_LEAN_AND_MEAN)
endif()

if(target MATCHES "win32-x64")
  enable_language(ASM_NASM)
else()
  enable_language(ASM)
endif()

set(BUILD_TESTING OFF)

fetch_package("github:google/boringssl#0.20260211.0")

set(certificates_bundle "${CMAKE_CURRENT_BINARY_DIR}/certificates.pem")

file(DOWNLOAD
  "https://curl.se/ca/cacert-2026-05-14.pem"
  "${certificates_bundle}"
  TLS_VERSION 1.2
  TLS_VERIFY ON
)

file(READ "${certificates_bundle}" certificates_data HEX)

string(REGEX REPLACE "([0-9a-f][0-9a-f])" "0x\\1," certificates_array "${certificates_data}")

set(certificates "${CMAKE_CURRENT_BINARY_DIR}/certificates.h")

configure_file(lib/certificates.h.in "${certificates}" @ONLY)

add_bare_module(bare_tls)

target_sources(
  ${bare_tls}
  PRIVATE
    binding.c
    "${certificates}"
)

target_link_libraries(
  ${bare_tls}
  PUBLIC
    ssl
)

target_include_directories(
  ${bare_tls}
  PRIVATE
    "${CMAKE_CURRENT_BINARY_DIR}"
)
