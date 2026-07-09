load("@rules_cc//cc:cc_library.bzl", "cc_library")

# @platforms//os:ios also matches the simulator; the device constraint
# disambiguates.
config_setting(
    name = "is_ios_device",
    constraint_values = [
        "@platforms//os:ios",
        "@apple_support//constraints:device",
    ],
)

cc_library(
    name = "v8",
    visibility = ["//visibility:public"],
    # Consumers check for this V8 build-time define (see
    # src/workerd/jsg/async-context.h); the bazel-built @v8//:v8 propagates it
    # unconditionally, but a raw cc_import doesn't know to.
    defines = select({
        ":is_ios_device": ["V8_ENABLE_CONTINUATION_PRESERVED_EMBEDDER_DATA"],
        "//conditions:default": [],
    }),
    deps = select({
        ":is_ios_device": ["@v8_ios_device_prebuilt//:v8"],
        "//conditions:default": [":v8_passthrough"],
    }),
)

# V8's bazel build doesn't support embedding ICU data, so workerd feeds it in
# itself on every platform built that way. The GN-built ios_arm64 import above
# has ICU compiled in natively and needs neither this define nor these deps.
cc_library(
    name = "v8_passthrough",
    defines = ["WORKERD_ICU_DATA_EMBED"],
    deps = [
        "@workerd//:icudata-embed",
        "@workerd//:v8_icu",
    ],
)
