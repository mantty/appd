set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR x86_64)

find_program(
  cc
  NAMES x86_64-linux-musl-cc
  REQUIRED
)

find_program(
  c++
  NAMES x86_64-linux-musl-c++
  REQUIRED
)

set(target x86_64-linux-musl)

set(CMAKE_C_COMPILER ${cc})
set(CMAKE_C_COMPILER_TARGET ${target})

set(CMAKE_CXX_COMPILER ${c++})
set(CMAKE_CXX_COMPILER_TARGET ${target})

set(CMAKE_ASM_COMPILER ${cc})
set(CMAKE_ASM_COMPILER_TARGET ${target})

set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(VCPKG_TARGET_TRIPLET x64-linux-musl)

add_compile_definitions(__MUSL__)
