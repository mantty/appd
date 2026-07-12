set(CMAKE_SYSTEM_NAME Darwin)
set(CMAKE_SYSTEM_PROCESSOR x86_64)

include("${CMAKE_CURRENT_LIST_DIR}/apple/find-clang.cmake")
include("${CMAKE_CURRENT_LIST_DIR}/apple/find-nasm.cmake")

set(target x86_64-apple-macosx13.0)

set(CMAKE_C_COMPILER ${clang})
set(CMAKE_C_COMPILER_TARGET ${target})

set(CMAKE_CXX_COMPILER ${clang++})
set(CMAKE_CXX_COMPILER_TARGET ${target})
set(CMAKE_CXX_COMPILER_CLANG_SCAN_DEPS ${clang-scan-deps})

set(CMAKE_ASM_COMPILER ${clang})
set(CMAKE_ASM_COMPILER_TARGET ${target})

if(nasm)
  set(CMAKE_ASM_NASM_COMPILER ${nasm})
endif()

set(CMAKE_OBJC_COMPILER ${clang})
set(CMAKE_OBJC_COMPILER_TARGET ${target})

set(CMAKE_OBJCXX_COMPILER ${clang++})
set(CMAKE_OBJCXX_COMPILER_TARGET ${target})

set(CMAKE_OSX_SYSROOT macosx)
set(CMAKE_OSX_DEPLOYMENT_TARGET 13.0)

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(VCPKG_TARGET_TRIPLET x64-osx)
