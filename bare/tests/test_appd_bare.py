from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))

import appd_bare


class BareBuildTests(unittest.TestCase):
    def test_upstream_pin_is_complete(self) -> None:
        config = appd_bare.load_upstream_config()
        self.assertEqual(config["tag"], "v2.3.0")
        self.assertEqual(len(config["commit"]), 40)
        self.assertEqual(len(config["source_sha256"]), 64)
        self.assertEqual(config["engine_repository"], "github:holepunchto/libjsc")
        self.assertEqual(len(config["engine_commit"]), 40)

    def test_generate_pins_javascriptcore(self) -> None:
        with mock.patch.object(appd_bare, "run_bare_make") as run:
            appd_bare.generate(
                Path("build"), Path("source"), "darwin", "arm64", Path("modules")
            )

        arguments = run.call_args.args
        self.assertIn(
            "BARE_ENGINE:STRING=github:holepunchto/libjsc#df04ed716bf0f8d5b9d4d4e6bccdacd1306f19a7",
            arguments,
        )

    def test_generate_uses_ios_deployment_target(self) -> None:
        with mock.patch.object(appd_bare, "run_bare_make") as run:
            appd_bare.generate(
                Path("build"), Path("source"), "ios", "arm64", Path("modules")
            )

        arguments = run.call_args.args
        self.assertIn("CMAKE_OSX_DEPLOYMENT_TARGET:STRING=17.0", arguments)

    def test_generate_uses_bare_default_engine_on_android(self) -> None:
        with mock.patch.object(appd_bare, "run_bare_make") as run:
            appd_bare.generate(
                Path("build"), Path("source"), "android", "arm64", Path("modules")
            )

        self.assertNotIn("BARE_ENGINE:STRING", run.call_args.args)

    def test_generate_marks_simulator_builds(self) -> None:
        with mock.patch.object(appd_bare, "run_bare_make") as run:
            appd_bare.generate(
                Path("build"), Path("source"), "ios", "arm64", Path("modules"), True
            )

        self.assertIn("--simulator", run.call_args.args)
        self.assertIn("APPLE_CLANG:BOOL=ON", run.call_args.args)

    def test_target_settings_cover_apple_targets(self) -> None:
        self.assertEqual(
            appd_bare.target_settings("ios-simulator-x64"), ("ios", "x64", True)
        )
        self.assertEqual(
            appd_bare.target_settings("macos-x64"), ("darwin", "x64", False)
        )

    def test_generate_uses_isolated_native_modules(self) -> None:
        with mock.patch.object(appd_bare, "run_bare_make") as run:
            appd_bare.generate(
                Path("build"), Path("source"), "darwin", "arm64", Path("modules")
            )

        self.assertIn("APPD_BARE_MODULES_ROOT:PATH=modules", run.call_args.args)

    def test_link_arguments_drop_compiler_and_output(self) -> None:
        command = "clang++ smoke.o -o smoke libappd.a -framework CoreFoundation && :"
        self.assertEqual(
            appd_bare.link_arguments(command),
            ["libappd.a", "-framework", "CoreFoundation"],
        )

    def test_force_loads_appd_bare(self) -> None:
        self.assertEqual(
            appd_bare.force_load_appd_bare(["libappd_bare.a", "-framework", "WebKit"]),
            ["-Xlinker", "-force_load", "-Xlinker", "libappd_bare.a", "-framework", "WebKit"],
        )

    def test_reads_cmake_cache_value(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "CMakeCache.txt"
            cache.write_text("CMAKE_MAKE_PROGRAM:FILEPATH=/tools/ninja\n")
            self.assertEqual(
                appd_bare.cmake_cache_value(cache, "CMAKE_MAKE_PROGRAM"),
                "/tools/ninja",
            )

    def test_copy_inputs_preserves_link_order(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build = root / "build"
            output = root / "sdk"
            build.mkdir()
            (build / "one.o").write_bytes(b"one")
            (build / "two.a").write_bytes(b"two")
            inputs = appd_bare.copy_link_inputs(build, output, ["one.o", "two.a", "one.o"])
            self.assertEqual([relative for _, relative in inputs], ["inputs/0000-one.o", "inputs/0001-two.a"])

    def test_package_sdk_writes_relocatable_manifest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            build = root / "build"
            output = root / "sdk"
            build.mkdir()
            (build / "libappd_bare.a").write_bytes(b"archive")
            command = "clang++ smoke.o -o smoke libappd_bare.a -framework JavaScriptCore"

            with (
                mock.patch.object(appd_bare, "link_command", return_value=command),
                mock.patch.object(appd_bare, "driver_link_arguments", return_value=["-lc++"]),
            ):
                appd_bare.package_sdk(build, output, "ios-arm64")

            manifest = json.loads((output / "sdk-manifest.json").read_text())
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["target"], "ios-arm64")
            self.assertEqual(manifest["engine"]["repository"], "github:holepunchto/libjsc")
            self.assertEqual(
                manifest["link_args"][:4],
                ["-Xlinker", "-force_load", "-Xlinker", "inputs/0000-libappd_bare.a"],
            )
            self.assertEqual(
                manifest["link_args"][4:],
                ["-framework", "JavaScriptCore", "-lc++"],
            )
            self.assertEqual(manifest["link_inputs"], [{"path": "inputs/0000-libappd_bare.a"}])


if __name__ == "__main__":
    unittest.main()
