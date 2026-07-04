import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OVERLAY_SOURCE = ROOT / "overlay" / "appd" / "embed" / "appd_workerd.cpp"
OVERLAY_BUILD = ROOT / "overlay" / "appd" / "embed" / "BUILD.bazel"
OVERLAY_ASPECT = ROOT / "overlay" / "appd" / "embed" / "link_inputs.bzl"


def read_overlay_source():
    return OVERLAY_SOURCE.read_text(encoding="utf-8")


def read_overlay_build():
    return OVERLAY_BUILD.read_text(encoding="utf-8")


class OverlayContractTests(unittest.TestCase):
    def test_listener_fd_guard_owns_socket_until_kj_takes_it(self):
        source = read_overlay_source()

        guard_class = source.index("class ListenerFdGuard")
        guard_instance = source.index("ListenerFdGuard listenerGuard(listenerFd);")
        wrap = source.index("wrapListenSocketFd(")
        release = source.index("listenerGuard.release();")
        first_startup_failure = source.index('config_path must not be null"')

        self.assertLess(guard_class, guard_instance)
        self.assertLess(guard_instance, first_startup_failure)
        self.assertLess(wrap, release)

    def test_config_const_schema_template_definition_is_included(self):
        source = read_overlay_source()

        self.assertIn("#include <capnp/dynamic.h>", source)
        self.assertIn("constSchema.as<server::config::Config>()", source)

    def test_overlay_source_uses_full_paths_for_relocated_workerd_headers(self):
        # appd_workerd.cpp lives outside src/workerd/server now, so it can no
        # longer reach server.h/v8-platform-impl.h via same-directory quotes.
        source = read_overlay_source()

        self.assertIn("#include <workerd/server/server.h>", source)
        self.assertIn("#include <workerd/server/v8-platform-impl.h>", source)
        self.assertNotIn('#include "server.h"', source)
        self.assertNotIn('#include "v8-platform-impl.h"', source)

    def test_build_defines_appd_workerd_as_a_public_plain_cc_library(self):
        build = read_overlay_build()

        self.assertIn('name = "appd-workerd"', build)
        self.assertIn('visibility = ["//visibility:public"]', build)
        # A plain cc_library, not workerd's own wd_cc_library macro -- this
        # package can't load workerd's build-internal .bzl files.
        self.assertIn('load("@rules_cc//cc:cc_library.bzl", "cc_library")', build)
        self.assertNotIn("wd_cc_library(", build)
        self.assertNotIn("appd-workerd-link-anchor", build)

    def test_aspect_exposes_the_documented_output_group(self):
        aspect = OVERLAY_ASPECT.read_text(encoding="utf-8")

        self.assertIn("link_inputs_aspect = aspect(", aspect)
        self.assertIn("appd_link_inputs", aspect)


if __name__ == "__main__":
    unittest.main()
