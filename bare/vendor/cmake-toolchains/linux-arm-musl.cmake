set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR arm)

find_program(
  cc
  NAMES arm-linux-musl-cc
  REQUIRED
)

find_program(
  c++
  NAMES arm-linux-musl-c++
  REQUIRED
)

set(target arm-linux-musl)

set(CMAKE_C_COMPILER ${cc})
set(CMAKE_C_COMPILER_TARGET ${target})

set(CMAKE_CXX_COMPILER ${c++})
set(CMAKE_CXX_COMPILER_TARGET ${target})

set(CMAKE_ASM_COMPILER ${cc})
set(CMAKE_ASM_COMPILER_TARGET ${target})

set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(VCPKG_TARGET_TRIPLET arm-linux-musl)

add_compile_definitions(__MUSL__)
