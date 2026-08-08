set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR mipsel)

find_program(
  cc
  NAMES mipsel-linux-muslsf-cc
  REQUIRED
)

find_program(
  c++
  NAMES mipsel-linux-muslsf-c++
  REQUIRED
)

set(target mipsel-linux-muslsf)

set(CMAKE_C_COMPILER ${cc})
set(CMAKE_C_COMPILER_TARGET ${target})

set(CMAKE_CXX_COMPILER ${c++})
set(CMAKE_CXX_COMPILER_TARGET ${target})

set(CMAKE_ASM_COMPILER ${cc})
set(CMAKE_ASM_COMPILER_TARGET ${target})

set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(VCPKG_TARGET_TRIPLET mipsel-linux-muslsf)

add_compile_definitions(__MUSL__)
