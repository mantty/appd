import importlib.util
import os
import sys
import textwrap
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import mock


def load_module():
    root = Path(__file__).resolve().parents[1]
    spec = importlib.util.spec_from_file_location(
        "appd_workerd", root / "scripts" / "appd_workerd.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules["appd_workerd"] = module
    spec.loader.exec_module(module)
    return module


# The slice of upstream's BUILD.bazel that widen_visibility() edits: four
# named targets plus a wd_capnp_library() call with no literal name.
FAKE_SERVER_BUILD = textwrap.dedent(
    """\
    cc_library(
        name = "server",
        srcs = ["server.c++"],
    )

    cc_library(
        name = "v8-platform-impl",
        srcs = ["v8-platform-impl.c++"],
    )

    cc_library(
        name = "cpp-capnp-schema",
        srcs = ["cpp-capnp-schema.c++"],
    )

    cc_library(
        name = "workerd-capnp-schema",
        srcs = ["workerd-capnp-schema.c++"],
    )

    wd_capnp_library(
        src = "workerd.capnp",
        visibility = [":__pkg__"],
    )
    """
)


class ApplyOverlayTests(unittest.TestCase):
    def test_copies_overlay_files_and_widens_visibility_idempotently(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            overlay = root / "overlay"

            (source / "src" / "workerd" / "server").mkdir(parents=True)
            # buildozer needs a workspace marker to resolve labels.
            (source / "MODULE.bazel").write_text('module(name = "workerd")\n', encoding="utf-8")
            (source / "src" / "workerd" / "server" / "BUILD.bazel").write_text(
                FAKE_SERVER_BUILD, encoding="utf-8"
            )
            (overlay / "appd" / "embed").mkdir(parents=True)
            (overlay / "appd" / "embed" / "appd_workerd.h").write_text(
                "#pragma once\n", encoding="utf-8"
            )

            workerd.apply_overlay(source, overlay)
            workerd.apply_overlay(source, overlay)

            self.assertEqual(
                (source / "appd" / "embed" / "appd_workerd.h").read_text(encoding="utf-8"),
                "#pragma once\n",
            )

            built = (source / "src" / "workerd" / "server" / "BUILD.bazel").read_text(
                encoding="utf-8"
            )
            for label in workerd.VISIBILITY_WIDENING_TARGETS:
                name = label.rsplit(":", 1)[-1]
                self.assertIn(f'name = "{name}"', built)
            self.assertEqual(built.count('visibility = ["//visibility:public"]'), 5)

    def test_v8_ios_device_dir_env_var_replaces_the_placeholder(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            overlay = root / "overlay"

            (source / "src" / "workerd" / "server").mkdir(parents=True)
            (source / "MODULE.bazel").write_text('module(name = "workerd")\n', encoding="utf-8")
            (source / "src" / "workerd" / "server" / "BUILD.bazel").write_text(
                FAKE_SERVER_BUILD, encoding="utf-8"
            )
            (overlay / "appd" / "embed").mkdir(parents=True)
            (overlay / "appd" / "embed" / "appd_workerd.h").write_text(
                "#pragma once\n", encoding="utf-8"
            )
            module_dir = overlay / "build" / "workerd-v8"
            module_dir.mkdir(parents=True)
            (module_dir / "MODULE.bazel").write_text(
                f'path = "{workerd.V8_IOS_DEVICE_DIR_PLACEHOLDER}",\n', encoding="utf-8"
            )

            module_path = source / workerd.V8_IOS_DEVICE_MODULE
            with mock.patch.dict(os.environ, {}, clear=False):
                os.environ.pop(workerd.V8_IOS_DEVICE_DIR_ENV, None)
                workerd.apply_overlay(source, overlay)
                self.assertEqual(
                    module_path.read_text(encoding="utf-8"),
                    f'path = "{workerd.V8_IOS_DEVICE_DIR_PLACEHOLDER}",\n',
                )

                os.environ[workerd.V8_IOS_DEVICE_DIR_ENV] = "/tmp/some-v8-checkout"
                workerd.apply_overlay(source, overlay)
                self.assertEqual(
                    module_path.read_text(encoding="utf-8"),
                    'path = "/tmp/some-v8-checkout",\n',
                )


if __name__ == "__main__":
    unittest.main()
