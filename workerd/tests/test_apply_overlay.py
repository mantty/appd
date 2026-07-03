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


class ApplyOverlayTests(unittest.TestCase):
    def test_copies_overlay_files_and_applies_ordered_patches(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            overlay = root / "overlay"
            patches = root / "patches"

            (source / "src" / "workerd" / "server").mkdir(parents=True)
            (source / "src" / "workerd" / "server" / "BUILD.bazel").write_text(
                "line one\nline two\n", encoding="utf-8"
            )
            (overlay / "src" / "workerd" / "server").mkdir(parents=True)
            (overlay / "src" / "workerd" / "server" / "appd_workerd.h").write_text(
                "#pragma once\n", encoding="utf-8"
            )
            patches.mkdir()
            (patches / "0001-test.patch").write_text(
                textwrap.dedent(
                    """\
                    --- a/src/workerd/server/BUILD.bazel
                    +++ b/src/workerd/server/BUILD.bazel
                    @@ -1,2 +1,3 @@
                     line one
                    +appd target
                     line two
                    """
                ),
                encoding="utf-8",
            )

            workerd.apply_overlay(source, overlay, patches)
            workerd.apply_overlay(source, overlay, patches)

            self.assertEqual(
                (source / "src" / "workerd" / "server" / "appd_workerd.h").read_text(
                    encoding="utf-8"
                ),
                "#pragma once\n",
            )
            self.assertEqual(
                (source / "src" / "workerd" / "server" / "BUILD.bazel").read_text(
                    encoding="utf-8"
                ),
                "line one\nappd target\nline two\n",
            )


if __name__ == "__main__":
    unittest.main()
