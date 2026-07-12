set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR AMD64)

find_program(
  cc
  NAMES
    x86_64-w64-mingw32-gcc
    x86_64-w64-mingw32-gcc.exe
  REQUIRED
)

find_program(
  c++
  NAMES
    x86_64-w64-mingw32-g++
    x86_64-w64-mingw32-g++.exe
  REQUIRED
)

set(target x86_64-w64-mingw32)

set(CMAKE_C_COMPILER ${cc})
set(CMAKE_C_COMPILER_TARGET ${target})

set(CMAKE_CXX_COMPILER ${c++})
set(CMAKE_CXX_COMPILER_TARGET ${target})

set(CMAKE_ASM_COMPILER ${cc})
set(CMAKE_ASM_COMPILER_TARGET ${target})

set(CMAKE_EXE_LINKER_FLAGS_INIT "-static")

set(CMAKE_POSITION_INDEPENDENT_CODE ON)

set(VCPKG_TARGET_TRIPLET x64-mingw)
