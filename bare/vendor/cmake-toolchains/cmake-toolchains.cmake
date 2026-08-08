if(CMAKE_TOOLCHAIN_FILE)
  get_filename_component(
    CMAKE_TOOLCHAIN_FILE
    "${CMAKE_TOOLCHAIN_FILE}"
    ABSOLUTE
  )

  set(
    CMAKE_TOOLCHAIN_FILE
    "${CMAKE_TOOLCHAIN_FILE}"
    CACHE FILEPATH "Toolchain file for the target build"
    FORCE
  )
endif()

if(CMAKE_HOST_SYSTEM_NAME STREQUAL "Darwin")
  set(host_platform darwin)
elseif(CMAKE_HOST_SYSTEM_NAME STREQUAL "Linux")
  set(host_platform linux)
elseif(CMAKE_HOST_SYSTEM_NAME STREQUAL "Windows")
  set(host_platform win32)
endif()

if(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(arm64|aarch64|ARM64)$")
  set(host_arch arm64)
elseif(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(x86_64|amd64|AMD64)$")
  set(host_arch x64)
elseif(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(i[3-6]86|x86)$")
  set(host_arch ia32)
elseif(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(armv7l|arm)$")
  set(host_arch arm)
elseif(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^riscv64$")
  set(host_arch riscv64)
endif()

if(DEFINED host_platform AND DEFINED host_arch)
  set(
    CMAKE_HOST_TOOLCHAIN_FILE
    "${CMAKE_CURRENT_LIST_DIR}/${host_platform}-${host_arch}.cmake"
    CACHE FILEPATH "Toolchain file for host-targeted build tools"
  )
endif()
