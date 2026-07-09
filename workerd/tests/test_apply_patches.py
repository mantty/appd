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


FAKE_TARGET_FILE = textwrap.dedent(
    """\
    TRIPLES = [
        "a",
        "b",
    ]
    """
)

FAKE_PATCH = textwrap.dedent(
    """\
    Add c to TRIPLES

    --- a/triples.bzl
    +++ b/triples.bzl
    @@ -1,4 +1,5 @@
     TRIPLES = [
         "a",
         "b",
    +    "c",
     ]
    """
)


class ApplyPatchesTests(unittest.TestCase):
    def test_applies_a_patch_and_is_idempotent(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            patches = root / "patches"

            source.mkdir()
            (source / "triples.bzl").write_text(FAKE_TARGET_FILE, encoding="utf-8")
            patches.mkdir()
            (patches / "0001-add-c.patch").write_text(FAKE_PATCH, encoding="utf-8")

            workerd.apply_patches(source, patches)
            workerd.apply_patches(source, patches)

            self.assertIn('"c",', (source / "triples.bzl").read_text(encoding="utf-8"))

    def test_missing_patches_directory_is_a_no_op(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            source = Path(temp_dir)
            workerd.apply_patches(source, source / "no-such-patches-dir")

    def test_reports_the_patch_name_and_conflict_on_failure(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            patches = root / "patches"

            source.mkdir()
            (source / "triples.bzl").write_text("TRIPLES = []\n", encoding="utf-8")
            patches.mkdir()
            (patches / "0001-add-c.patch").write_text(FAKE_PATCH, encoding="utf-8")

            with self.assertRaisesRegex(RuntimeError, "0001-add-c.patch"):
                workerd.apply_patches(source, patches)


if __name__ == "__main__":
    unittest.main()
