import importlib.util
import sys
import textwrap
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory


def load_module():
    root = Path(__file__).resolve().parents[1]
    spec = importlib.util.spec_from_file_location(
        "appd_workerd", root / "scripts" / "appd_workerd.py"
    )
    module = importlib.util.module_from_spec(spec)
    sys.modules["appd_workerd"] = module
    spec.loader.exec_module(module)
    return module


# A stand-in for the slice of upstream's real src/workerd/server/BUILD.bazel
# that widen_visibility() needs to edit: four plainly-named targets, plus a
# wd_capnp_library(src = "workerd.capnp", ...) macro call that (like the real
# one) never writes a literal `name = "workerd_capnp"`.
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
            # buildozer walks up from -root_dir looking for a workspace
            # marker (like real Bazel does) to resolve package-relative
            # labels; the real fetched source has one, so this needs one too.
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


if __name__ == "__main__":
    unittest.main()
