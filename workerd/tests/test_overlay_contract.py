import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
OVERLAY_SOURCE = ROOT / "overlay" / "src" / "workerd" / "server" / "appd_workerd.cpp"
PATCHES_DIR = ROOT / "patches"


def read_overlay_source():
    return OVERLAY_SOURCE.read_text(encoding="utf-8")


def read_patches():
    return "\n".join(
        path.read_text(encoding="utf-8") for path in sorted(PATCHES_DIR.glob("*.patch"))
    )


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

    def test_overlay_keeps_appd_target_patch_without_disk_kv_patch(self):
        patches = read_patches()

        self.assertIn('name = "appd-workerd"', patches)
        self.assertIn('name = "appd-workerd-link-anchor"', patches)
        self.assertNotIn("When used as a KV backend", patches)
        self.assertNotIn("remaining.findFirst('/')", patches)
        self.assertNotIn("url.path = fixedPath.releaseAsArray();", patches)


if __name__ == "__main__":
    unittest.main()
