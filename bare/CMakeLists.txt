cmake_minimum_required(VERSION 4.0)

project(appd_bare LANGUAGES C CXX VERSION 0.1.0)

if(NOT DEFINED BARE_KIT_SOURCE)
  message(FATAL_ERROR "BARE_KIT_SOURCE must point to the pinned BareKit source")
endif()
if(NOT DEFINED APPD_BARE_MODULES_ROOT)
  message(FATAL_ERROR "APPD_BARE_MODULES_ROOT must point to the isolated native module deployment")
endif()

foreach(package cmake-bare cmake-bare-bundle cmake-fetch cmake-java cmake-napi cmake-npm)
  set("${package}_DIR" "${BARE_KIT_SOURCE}/node_modules/${package}")
endforeach()
add_subdirectory("${BARE_KIT_SOURCE}" "${CMAKE_BINARY_DIR}/bare-kit")

if(ANDROID)
  add_library(appd_bare SHARED)
else()
  add_library(appd_bare STATIC)
endif()

set_target_properties(
  appd_bare
  PROPERTIES
  C_STANDARD 11
  POSITION_INDEPENDENT_CODE ON
)

target_include_directories(appd_bare SYSTEM PUBLIC "${BARE_KIT_SOURCE}/shared")
target_link_libraries(appd_bare PUBLIC bare_worklet)

link_bare_modules(appd_bare WORKING_DIRECTORY "${APPD_BARE_MODULES_ROOT}")

add_executable(appd_bare_link_test tests/link.c)
target_link_libraries(appd_bare_link_test PRIVATE appd_bare)

if(NOT ANDROID)
  target_link_libraries(appd_bare_link_test PRIVATE System)
endif()

if(APPLE)
  execute_process(
    COMMAND xcrun --find clang
    OUTPUT_VARIABLE xcrun_clang
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  execute_process(
    COMMAND "${xcrun_clang}" --print-resource-dir
    OUTPUT_VARIABLE compiler_resource_dir
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  set(compiler_runtime "${compiler_resource_dir}/lib/darwin/libclang_rt.${CMAKE_SYSTEM_NAME}.a")
  if(CMAKE_SYSTEM_NAME STREQUAL "Darwin")
    set(compiler_runtime "${compiler_resource_dir}/lib/darwin/libclang_rt.osx.a")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "iOS")
    if(CMAKE_C_COMPILER_TARGET MATCHES "simulator")
      set(compiler_runtime "${compiler_resource_dir}/lib/darwin/libclang_rt.iossim.a")
    else()
      set(compiler_runtime "${compiler_resource_dir}/lib/darwin/libclang_rt.ios.a")
    endif()
  endif()
  target_link_libraries(appd_bare_link_test PRIVATE "${compiler_runtime}")
endif()

install(TARGETS appd_bare ARCHIVE LIBRARY DESTINATION lib)
