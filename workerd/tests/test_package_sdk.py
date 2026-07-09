import importlib.util
import json
import sys
import unittest
from unittest import mock
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


class PackageSdkTests(unittest.TestCase):
    def test_default_bazel_args_are_known_for_supported_cargo_triples_and_aliases(self):
        workerd = load_module()
        drumbrake = "--@v8//:v8_enable_drumbrake=true"

        self.assertEqual(
            workerd.default_bazel_args("x86_64-unknown-linux-gnu"),
            [drumbrake, "--config=release_linux"],
        )
        self.assertEqual(
            workerd.default_bazel_args("linux-x64"),
            [drumbrake, "--config=release_linux"],
        )
        self.assertEqual(
            workerd.default_bazel_args("aarch64-apple-darwin"),
            [drumbrake, "--config=release_macos"],
        )
        self.assertEqual(
            workerd.default_bazel_args("macos-arm64"),
            [drumbrake, "--config=release_macos"],
        )
        self.assertEqual(
            workerd.default_bazel_args("ios-simulator-arm64"),
            [drumbrake, "--config=release_macos"],
        )
        self.assertEqual(
            workerd.default_bazel_args(
                "x86_64-apple-darwin",
                host_system="Darwin",
                host_machine="arm64",
            ),
            [drumbrake, "--config=release_macos_cross_x86_64"],
        )
        self.assertEqual(
            workerd.default_bazel_args(
                "x86_64-apple-darwin",
                host_system="Darwin",
                host_machine="x86_64",
            ),
            [drumbrake, "--config=release_macos"],
        )
        self.assertEqual(
            workerd.default_bazel_args(
                "macos-x64",
                host_system="Darwin",
                host_machine="arm64",
            ),
            [drumbrake, "--config=release_macos_cross_x86_64"],
        )
        self.assertEqual(
            workerd.default_bazel_args(
                "ios-simulator-x64",
                host_system="Darwin",
                host_machine="arm64",
            ),
            [drumbrake, "--config=release_macos_cross_x86_64"],
        )
        self.assertEqual(
            workerd.default_bazel_args(
                "ios-simulator-x64",
                host_system="Darwin",
                host_machine="x86_64",
            ),
            [drumbrake, "--config=release_macos"],
        )
        self.assertEqual(
            workerd.default_bazel_args("x86_64-pc-windows-msvc"),
            [drumbrake, "--config=release_windows"],
        )
        self.assertEqual(
            workerd.default_bazel_args("windows-x64"),
            [drumbrake, "--config=release_windows"],
        )
        self.assertEqual(
            workerd.default_bazel_args("aarch64-apple-ios"),
            [
                drumbrake,
                "--config=release",
                "--platforms=@apple_support//platforms:ios_arm64",
                "--//build/config:tool_is_executable=false",
            ],
        )
        self.assertEqual(
            workerd.default_bazel_args("ios-arm64"),
            [
                drumbrake,
                "--config=release",
                "--platforms=@apple_support//platforms:ios_arm64",
                "--//build/config:tool_is_executable=false",
            ],
        )
        self.assertEqual(workerd.normalize_target("macos-arm64"), "aarch64-apple-darwin")
        self.assertEqual(workerd.normalize_target("ios-simulator-x64"), "x86_64-apple-ios")
        self.assertEqual(workerd.normalize_target("ios-arm64"), "aarch64-apple-ios")

        with self.assertRaisesRegex(ValueError, "no default Bazel configuration"):
            workerd.default_bazel_args("android-arm64")

    def test_build_sdk_normalizes_target_and_adds_local_cache_args(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            source = root / "source"
            output = root / "out"
            cache = root / "cache"
            config = root / "upstream.toml"
            params = (
                source
                / "bazel-bin"
                / "appd"
                / "embed"
                / "libappd-workerd-appd-link-inputs.dylib-0.params"
            )
            header = source / "appd" / "embed" / "appd_workerd.h"
            lib = source / "bazel-bin" / "libappd.a"

            header.parent.mkdir(parents=True)
            params.parent.mkdir(parents=True)
            lib.parent.mkdir(parents=True, exist_ok=True)
            header.write_text("#pragma once\n", encoding="utf-8")
            lib.write_bytes(b"archive")
            params.write_text(str(lib) + "\n", encoding="utf-8")
            config.write_text(
                "\n".join(
                    [
                        "[upstream]",
                        'repository = "https://github.com/cloudflare/workerd"',
                        'tag = "v1"',
                        'commit = "abc"',
                        'source_url = "https://example.invalid/workerd.tar.gz"',
                        'source_sha256 = "0"',
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            with (
                mock.patch.object(workerd, "apply_overlay"),
                mock.patch.object(workerd, "apply_patches"),
                mock.patch.object(workerd.subprocess, "run") as run,
            ):
                workerd.build_sdk(
                    target="macos-arm64",
                    output_dir=output,
                    config_path=config,
                    source_dir=source,
                    bazel_args=["--verbose_failures"],
                    cache_config=workerd.BuildCacheConfig(mode="local", cache_dir=cache),
                )

            self.assertEqual(
                run.call_args.args[0],
                [
                    "bazel",
                    "build",
                    f"--aspects={workerd.APPD_LINK_INPUTS_ASPECT}",
                    f"--output_groups={workerd.APPD_LINK_INPUTS_OUTPUT_GROUP}",
                    workerd.APPD_BAZEL_TARGET,
                    "--@v8//:v8_enable_drumbrake=true",
                    "--config=release_macos",
                    f"--disk_cache={cache / 'bazel-disk'}",
                    f"--repository_cache={cache / 'bazel-repository'}",
                    "--verbose_failures",
                ],
            )
            manifest = json.loads((output / "sdk-manifest.json").read_text(encoding="utf-8"))
            self.assertEqual(manifest["target"], "aarch64-apple-darwin")

    def test_r2_cache_args_use_local_bazel_remote_and_control_uploads(self):
        workerd = load_module()
        settings = workerd.R2CacheSettings(
            endpoint="https://account.r2.cloudflarestorage.com",
            bucket="appd-workerd-bazel-cache",
            prefix="workerd",
            access_key_id="key-id",
            secret_access_key="secret-value",
        )
        cache = workerd.BuildCacheConfig(
            mode="r2-read",
            cache_dir=Path("/tmp/appd-cache"),
            r2=settings,
        )

        self.assertEqual(
            workerd.bazel_cache_args(cache, "http://127.0.0.1:9090"),
            [
                "--disk_cache=/tmp/appd-cache/bazel-disk",
                "--repository_cache=/tmp/appd-cache/bazel-repository",
                "--remote_cache=http://127.0.0.1:9090",
                "--remote_upload_local_results=false",
            ],
        )

        cache = workerd.BuildCacheConfig(
            mode="r2-read-write",
            cache_dir=Path("/tmp/appd-cache"),
            r2=settings,
        )
        self.assertIn(
            "--remote_upload_local_results=true",
            workerd.bazel_cache_args(cache, "http://127.0.0.1:9090"),
        )

    def test_bazel_s3_credentials_enable_shared_r2_cache_by_default(self):
        workerd = load_module()

        cache = workerd.cache_config_from_env(
            env={
                "APPD_BAZEL_S3_ACCESS_KEY_ID": "key-id",
                "APPD_BAZEL_S3_SECRET_ACCESS_KEY": "secret-value",
            }
        )

        self.assertEqual(cache.mode, "r2-read-write")
        self.assertEqual(
            cache.r2.endpoint,
            "https://dacf3ead71e534fdef9555c28d81774c.r2.cloudflarestorage.com",
        )
        self.assertEqual(cache.r2.bucket, "appd-workerd-bazel-cache")
        self.assertEqual(cache.r2.prefix, "bazel/appd-workerd")
        self.assertEqual(cache.r2.access_key_id, "key-id")
        self.assertEqual(cache.r2.secret_access_key, "secret-value")

    def test_old_r2_environment_names_do_not_enable_shared_cache(self):
        workerd = load_module()

        cache = workerd.cache_config_from_env(
            env={
                "APPD_R2_ACCESS_KEY_ID": "key-id",
                "APPD_R2_SECRET_ACCESS_KEY": "secret-value",
            }
        )

        self.assertEqual(cache.mode, "local")
        self.assertIsNone(cache.r2)

    def test_bazel_remote_environment_keeps_r2_secret_in_environment(self):
        workerd = load_module()
        settings = workerd.R2CacheSettings(
            endpoint="https://account.r2.cloudflarestorage.com",
            bucket="appd-workerd-bazel-cache",
            prefix="workerd",
            access_key_id="key-id",
            secret_access_key="secret-value",
            session_token="session-value",
        )
        cache = workerd.BuildCacheConfig(
            mode="r2-read-write",
            cache_dir=Path("/tmp/appd-cache"),
            bazel_remote_bin="bazel-remote",
            max_size_gib=8,
            r2=settings,
        )

        environment = workerd.bazel_remote_environment(cache, 9090)

        self.assertEqual(environment["BAZEL_REMOTE_S3_SECRET_ACCESS_KEY"], "secret-value")
        self.assertEqual(environment["BAZEL_REMOTE_S3_SESSION_TOKEN"], "session-value")
        self.assertEqual(environment["BAZEL_REMOTE_S3_BUCKET"], "appd-workerd-bazel-cache")
        self.assertEqual(environment["BAZEL_REMOTE_S3_BUCKET_LOOKUP_TYPE"], "path")

    def test_packages_link_inputs_and_excludes_introspection_artifacts(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            bazel_bin = root / "bazel-bin"
            output = root / "out"
            include = root / "appd_workerd.h"
            lib_a = bazel_bin / "appd" / "embed" / "libappd-workerd.a"
            lib_b = bazel_bin / "external" / "v8" / "libv8.a"
            lib_lo = bazel_bin / "external" / "v8" / "libv8_icu.lo"
            # The link-inputs aspect's own throwaway dylib and its LTO object
            # -- neither is a real appd-workerd dependency.
            lto_object = bazel_bin / "appd" / "embed" / "libappd-workerd-appd-link-inputs.dylib.lto.o"

            lib_a.parent.mkdir(parents=True)
            lib_b.parent.mkdir(parents=True)
            lib_lo.parent.mkdir(parents=True, exist_ok=True)
            lib_a.write_bytes(b"appd archive")
            lib_b.write_bytes(b"v8 archive")
            lib_lo.write_bytes(b"v8 lo archive")
            lto_object.write_bytes(b"lto summary")
            include.write_text("#pragma once\n", encoding="utf-8")

            dylib = "bazel-out/darwin_arm64-opt/bin/appd/embed/libappd-workerd-appd-link-inputs.dylib"
            params = root / "appd-link-inputs.params"
            params.write_text(
                "\n".join(
                    [
                        "-shared",
                        "-o",
                        dylib,
                        f"LINKED_BINARY={dylib}",
                        "-Xlinker",
                        "-object_path_lto",
                        "-Xlinker",
                        "bazel-out/darwin_arm64-opt/bin/appd/embed/libappd-workerd-appd-link-inputs.dylib.lto.o",
                        f"-Wl,-force_load,{lib_a}",
                        f"-Wl,-force_load,{lib_lo}",
                        str(lib_b),
                        str(lto_object),
                        "-Xlinker",
                        "-install_name",
                        "-Xlinker",
                        "@rpath/libappd-workerd-appd-link-inputs.dylib",
                    ]
                )
                + "\n",
                encoding="utf-8",
            )

            manifest_path = workerd.package_sdk(
                params_path=params,
                output_dir=output,
                target="aarch64-apple-darwin",
                upstream_tag="v1.20260501.1",
                upstream_commit="37ff673e9df2b628533da9a221d97ca54436d9e2",
                header_path=include,
            )

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["target"], "aarch64-apple-darwin")
            self.assertEqual(manifest["upstream"]["tag"], "v1.20260501.1")
            self.assertEqual(len(manifest["link_inputs"]), 3)
            self.assertTrue((output / "include" / "appd_workerd.h").is_file())
            self.assertFalse(any("appd-link-inputs" in item["source"] for item in manifest["link_inputs"]))
            self.assertNotIn("-shared", manifest["link_args"])
            self.assertNotIn("-o", manifest["link_args"])
            self.assertNotIn("-object_path_lto", manifest["link_args"])
            self.assertNotIn("-install_name", manifest["link_args"])
            self.assertFalse(any(arg.startswith("LINKED_BINARY=") for arg in manifest["link_args"]))
            self.assertFalse(any("bazel-out/" in arg for arg in manifest["link_args"]))
            self.assertTrue(any(arg.startswith("-Wl,-force_load,lib/") for arg in manifest["link_args"]))
            self.assertTrue(all((output / item["path"]).is_file() for item in manifest["link_inputs"]))
            self.assertTrue(all("appd-link-inputs" not in arg for arg in manifest["link_args"]))

    def test_packages_bazel_root_relative_link_inputs(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            output = root / "out"
            include = root / "appd_workerd.h"
            params = root / "bazel-bin" / "src" / "workerd" / "server" / "anchor.params"
            lib = (
                root
                / "bazel-out"
                / "darwin_arm64-opt"
                / "bin"
                / "src"
                / "workerd"
                / "server"
                / "libappd-workerd.a"
            )

            (root / "MODULE.bazel").write_text('module(name = "workerd")\n', encoding="utf-8")
            params.parent.mkdir(parents=True)
            lib.parent.mkdir(parents=True)
            include.write_text("#pragma once\n", encoding="utf-8")
            lib.write_bytes(b"appd archive")
            params.write_text(
                "bazel-out/darwin_arm64-opt/bin/src/workerd/server/libappd-workerd.a\n",
                encoding="utf-8",
            )

            manifest_path = workerd.package_sdk(
                params_path=params,
                output_dir=output,
                target="aarch64-apple-darwin",
                upstream_tag="v1.20260501.1",
                upstream_commit="37ff673e9df2b628533da9a221d97ca54436d9a",
                header_path=include,
            )

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(len(manifest["link_inputs"]), 1)
            self.assertTrue((output / manifest["link_inputs"][0]["path"]).is_file())

    def test_packages_execroot_relative_link_inputs_from_external_repos(self):
        workerd = load_module()
        with TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            output = root / "out"
            include = root / "appd_workerd.h"
            params = root / "bazel-bin" / "appd" / "embed" / "anchor.params"
            execroot = root / "output_base" / "execroot" / "_main"
            lib = execroot / "external" / "some_repo" / "libv8_monolith.a"

            (root / "MODULE.bazel").write_text('module(name = "workerd")\n', encoding="utf-8")
            params.parent.mkdir(parents=True)
            lib.parent.mkdir(parents=True)
            include.write_text("#pragma once\n", encoding="utf-8")
            lib.write_bytes(b"v8 archive")
            # bazel-out is a fixed-name convenience symlink into the execroot;
            # a local_repository-based external dep only resolves through it.
            (root / "bazel-out").symlink_to(execroot / "bazel-out", target_is_directory=True)
            params.write_text(
                "external/some_repo/libv8_monolith.a\n",
                encoding="utf-8",
            )

            manifest_path = workerd.package_sdk(
                params_path=params,
                output_dir=output,
                target="aarch64-apple-ios",
                upstream_tag="v1.20260501.1",
                upstream_commit="37ff673e9df2b628533da9a221d97ca54436d9b",
                header_path=include,
            )

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(len(manifest["link_inputs"]), 1)
            self.assertTrue((output / manifest["link_inputs"][0]["path"]).is_file())


if __name__ == "__main__":
    unittest.main()
