if(NOT DEFINED ENV{ANDROID_HOME})
  message(
    FATAL_ERROR
    "The ANDROID_HOME environment variable must be set for the Android "
    "toolchain to find a usable NDK. See "
    "https://developer.android.com/tools/variables#android_home"
  )
endif()

set(ANDROID_HOME "$ENV{ANDROID_HOME}")

if(DEFINED ENV{ANDROID_NDK_LATEST_HOME})
  set(ANDROID_NDK "$ENV{ANDROID_NDK_LATEST_HOME}")
elseif(DEFINED ENV{ANDROID_NDK_HOME})
  set(ANDROID_NDK "$ENV{ANDROID_NDK_HOME}")
else()
  file(GLOB ANDROID_NDK_CANDIDATES RELATIVE "${ANDROID_HOME}/ndk" "${ANDROID_HOME}/ndk/*")

  list(FILTER ANDROID_NDK_CANDIDATES INCLUDE REGEX "^[0-9]+\.[0-9]+\.[0-9]+$")

  if(NOT ANDROID_NDK_CANDIDATES)
    message(
      FATAL_ERROR
      "No usable Android NDK was found in ANDROID_HOME. See "
      "https://developer.android.com/studio/projects/install-ndk"
    )
  endif()

  list(SORT ANDROID_NDK_CANDIDATES COMPARE NATURAL ORDER DESCENDING)

  list(POP_FRONT ANDROID_NDK_CANDIDATES ANDROID_NDK_VERSION)

  set(ANDROID_NDK "${ANDROID_HOME}/ndk/${ANDROID_NDK_VERSION}")

  set(ENV{ANDROID_NDK_HOME} "${ANDROID_NDK}")
endif()
