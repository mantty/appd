import importlib.util
import json
import shutil
import struct
import subprocess
import sys
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


@unittest.skipUnless(shutil.which("ar"), "ar(1) is required to build/inspect archives")
class RetagTests(unittest.TestCase):
    @staticmethod
    def fake_macho_object(workerd, platform: int) -> bytes:
        """A minimal Mach-O64 object: just enough header plus a single
        LC_BUILD_VERSION load command for retag_object() to find and rewrite."""
        header = struct.pack(
            "<IiiiiiII",
            workerd.MACHO_MAGIC_64,  # magic
            0x0100000C,  # cputype: CPU_TYPE_ARM64
            0,  # cpusubtype
            1,  # filetype: MH_OBJECT
            1,  # ncmds
            24,  # sizeofcmds
            0,  # flags
            0,  # reserved
        )
        load_command = struct.pack(
            "<IIIIII",
            workerd.LC_BUILD_VERSION,  # cmd
            24,  # cmdsize
            platform,
            0,  # minos
            0,  # sdk
            0,  # ntools
        )
        return header + load_command

    @staticmethod
    def platform_of(workerd, macho_bytes: bytes) -> int:
        magic, _, _, _, ncmds = struct.unpack_from("<IiiiI", macho_bytes, 0)
        assert magic == workerd.MACHO_MAGIC_64
        offset = 32
        for _ in range(ncmds):
            cmd, cmdsize = struct.unpack_from("<II", macho_bytes, offset)
            if cmd == workerd.LC_BUILD_VERSION:
                (platform,) = struct.unpack_from("<I", macho_bytes, offset + 8)
                return platform
            offset += cmdsize
        raise AssertionError("no LC_BUILD_VERSION command found")
    def test_retag_object_rewrites_matching_platform_in_place(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "object.o"
            path.write_bytes(self.fake_macho_object(workerd, workerd.MACHO_PLATFORMS["macos"]))

            changed = workerd.retag_object(
                path, workerd.MACHO_PLATFORMS["macos"], workerd.MACHO_PLATFORMS["ios-simulator"]
            )

            self.assertTrue(changed)
            self.assertEqual(
                self.platform_of(workerd, path.read_bytes()),
                workerd.MACHO_PLATFORMS["ios-simulator"],
            )

    def test_retag_object_leaves_non_matching_platform_untouched(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "object.o"
            path.write_bytes(self.fake_macho_object(workerd, workerd.MACHO_PLATFORMS["ios-simulator"]))

            changed = workerd.retag_object(
                path, workerd.MACHO_PLATFORMS["macos"], workerd.MACHO_PLATFORMS["ios-simulator"]
            )

            self.assertFalse(changed)
            self.assertEqual(
                self.platform_of(workerd, path.read_bytes()),
                workerd.MACHO_PLATFORMS["ios-simulator"],
            )

    def test_retag_object_ignores_non_macho_files(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "object.o"
            data = b"not a mach-o file"
            path.write_bytes(data)

            changed = workerd.retag_object(
                path, workerd.MACHO_PLATFORMS["macos"], workerd.MACHO_PLATFORMS["ios-simulator"]
            )

            self.assertFalse(changed)
            self.assertEqual(path.read_bytes(), data)

    def test_retag_archive_rewrites_every_macho_member(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            build_dir = Path(temp_dir) / "build"
            build_dir.mkdir()
            for name in ("one.o", "two.o"):
                (build_dir / name).write_bytes(
                    self.fake_macho_object(workerd, workerd.MACHO_PLATFORMS["macos"])
                )

            archive = Path(temp_dir) / "library.a"
            subprocess.run(
                ["ar", "rcs", str(archive), "one.o", "two.o"], cwd=build_dir, check=True
            )

            workerd.retag_link_input(archive, "ios-simulator")

            with TemporaryDirectory() as extract_dir:
                subprocess.run(["ar", "x", str(archive)], cwd=extract_dir, check=True)
                for name in ("one.o", "two.o"):
                    member_bytes = (Path(extract_dir) / name).read_bytes()
                    self.assertEqual(
                        self.platform_of(workerd, member_bytes),
                        workerd.MACHO_PLATFORMS["ios-simulator"],
                    )

    def test_retag_archive_leaves_unparseable_archive_untouched(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            archive = Path(temp_dir) / "library.a"
            data = b"not an ar archive"
            archive.write_bytes(data)

            workerd.retag_link_input(archive, "ios-simulator")

            self.assertEqual(archive.read_bytes(), data)

    def test_package_sdk_retags_link_inputs_for_ios_simulator_target(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            build_dir = root / "build"
            build_dir.mkdir()
            (build_dir / "appd_workerd.o").write_bytes(
                self.fake_macho_object(workerd, workerd.MACHO_PLATFORMS["macos"])
            )
            lib = root / "bazel-bin" / "libappd-workerd.a"
            lib.parent.mkdir(parents=True)
            subprocess.run(
                ["ar", "rcs", str(lib), "appd_workerd.o"], cwd=build_dir, check=True
            )

            include = root / "appd_workerd.h"
            include.write_text("#pragma once\n", encoding="utf-8")
            params = root / "appd-link-inputs.params"
            params.write_text(str(lib) + "\n", encoding="utf-8")

            output = root / "out"
            manifest_path = workerd.package_sdk(
                params_path=params,
                output_dir=output,
                target="aarch64-apple-ios-sim",
                upstream_tag="v1.20260501.1",
                upstream_commit="37ff673e9df2b628533da9a221d97ca54436d9e2",
                header_path=include,
            )

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            packaged = output / manifest["link_inputs"][0]["path"]
            with TemporaryDirectory() as extract_dir:
                subprocess.run(["ar", "x", str(packaged)], cwd=extract_dir, check=True)
                member_bytes = (Path(extract_dir) / "appd_workerd.o").read_bytes()
                self.assertEqual(
                    self.platform_of(workerd, member_bytes),
                    workerd.MACHO_PLATFORMS["ios-simulator"],
                )
            # The manifest hashes the retagged bytes actually shipped, not
            # the pre-retag bytes copied out of bazel-bin.
            self.assertEqual(
                manifest["link_inputs"][0]["sha256"], workerd.sha256_file(packaged)
            )

    def test_package_sdk_does_not_retag_macos_target(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            build_dir = root / "build"
            build_dir.mkdir()
            (build_dir / "appd_workerd.o").write_bytes(
                self.fake_macho_object(workerd, workerd.MACHO_PLATFORMS["macos"])
            )
            lib = root / "bazel-bin" / "libappd-workerd.a"
            lib.parent.mkdir(parents=True)
            subprocess.run(
                ["ar", "rcs", str(lib), "appd_workerd.o"], cwd=build_dir, check=True
            )

            include = root / "appd_workerd.h"
            include.write_text("#pragma once\n", encoding="utf-8")
            params = root / "appd-link-inputs.params"
            params.write_text(str(lib) + "\n", encoding="utf-8")

            output = root / "out"
            manifest_path = workerd.package_sdk(
                params_path=params,
                output_dir=output,
                target="aarch64-apple-darwin",
                upstream_tag="v1.20260501.1",
                upstream_commit="37ff673e9df2b628533da9a221d97ca54436d9e2",
                header_path=include,
            )

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            packaged = output / manifest["link_inputs"][0]["path"]
            with TemporaryDirectory() as extract_dir:
                subprocess.run(["ar", "x", str(packaged)], cwd=extract_dir, check=True)
                member_bytes = (Path(extract_dir) / "appd_workerd.o").read_bytes()
                self.assertEqual(
                    self.platform_of(workerd, member_bytes),
                    workerd.MACHO_PLATFORMS["macos"],
                )


if __name__ == "__main__":
    unittest.main()
