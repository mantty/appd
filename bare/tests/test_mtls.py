from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OPENSSL = shutil.which("openssl")


def run_openssl(*arguments: str, cwd: Path) -> None:
    subprocess.run([OPENSSL, *arguments], cwd=cwd, check=True, capture_output=True, text=True)


def certificate_date(value: datetime) -> str:
    return value.strftime("%y%m%d%H%M%SZ")


def make_ca(directory: Path, name: str) -> tuple[Path, Path, Path]:
    key = directory / f"{name}.key"
    csr = directory / f"{name}.csr"
    certificate = directory / f"{name}.pem"
    config = directory / f"{name}.conf"
    (directory / f"{name}-newcerts").mkdir()
    (directory / f"{name}-index.txt").touch()
    (directory / f"{name}-serial").write_text("1000\n")
    config.write_text(
        "\n".join(
            [
                "[ca]",
                "default_ca = CA_default",
                "[CA_default]",
                f"database = {directory / f'{name}-index.txt'}",
                f"new_certs_dir = {directory / f'{name}-newcerts'}",
                f"serial = {directory / f'{name}-serial'}",
                f"certificate = {certificate}",
                f"private_key = {key}",
                "default_md = sha256",
                "policy = policy",
                "[policy]",
                "commonName = supplied",
                "[ca_ext]",
                "basicConstraints = critical,CA:TRUE",
                "keyUsage = critical,keyCertSign,cRLSign",
                "",
            ]
        )
    )
    run_openssl("ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(key), cwd=directory)
    run_openssl("req", "-new", "-key", str(key), "-out", str(csr), "-subj", f"/CN={name}", cwd=directory)
    run_openssl(
        "ca",
        "-batch",
        "-selfsign",
        "-config",
        str(config),
        "-keyfile",
        str(key),
        "-in",
        str(csr),
        "-out",
        str(certificate),
        "-startdate",
        "20000101000000Z",
        "-enddate",
        "20400101000000Z",
        "-extensions",
        "ca_ext",
        cwd=directory,
    )
    return key, certificate, config


def make_leaf(
    directory: Path,
    name: str,
    ca: tuple[Path, Path, Path],
    usage: str,
    san: str | None = None,
    expired: bool = False,
) -> tuple[Path, Path]:
    ca_key, ca_certificate, ca_config = ca
    key = directory / f"{name}.key"
    csr = directory / f"{name}.csr"
    certificate = directory / f"{name}.pem"
    extensions = directory / f"{name}.ext"
    extensions.write_text(
        "\n".join(
            [
                "[leaf]",
                "basicConstraints = critical,CA:FALSE",
                "keyUsage = critical,digitalSignature,keyEncipherment",
                f"extendedKeyUsage = {usage}",
                *([] if san is None else [f"subjectAltName = {san}"]),
                "",
            ]
        )
    )
    run_openssl("ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(key), cwd=directory)
    run_openssl("req", "-new", "-key", str(key), "-out", str(csr), "-subj", f"/CN={name}", cwd=directory)
    arguments = [
        "ca",
        "-batch",
        "-config",
        str(ca_config),
        "-keyfile",
        str(ca_key),
        "-cert",
        str(ca_certificate),
        "-in",
        str(csr),
        "-out",
        str(certificate),
        "-extfile",
        str(extensions),
        "-extensions",
        "leaf",
    ]
    now = datetime.now(timezone.utc)
    if expired:
        arguments.extend(["-startdate", "20000101000000Z", "-enddate", "20010101000000Z"])
    else:
        arguments.extend(
            [
                "-startdate",
                certificate_date(now - timedelta(days=1)),
                "-enddate",
                certificate_date(now + timedelta(days=30)),
            ]
        )
    run_openssl(*arguments, cwd=directory)
    return key, certificate


def create_certificates(directory: Path) -> None:
    ca = make_ca(directory, "ca")
    wrong_ca = make_ca(directory, "wrong-ca")
    make_leaf(directory, "server", ca, "serverAuth", "DNS:localhost")
    make_leaf(directory, "server-wrong-host", ca, "serverAuth", "DNS:other")
    make_leaf(directory, "server-expired", ca, "serverAuth", "DNS:localhost", expired=True)
    make_leaf(directory, "client", ca, "clientAuth")
    make_leaf(directory, "client-wrong-ca", wrong_ca, "clientAuth")


def stage_runtime(directory: Path, artifact: Path) -> Path:
    modules = directory / "node_modules"
    modules.mkdir()
    source = ROOT / "vendor/bare-tls"
    destination = modules / "bare-tls"
    shutil.copytree(source, destination)
    prebuild = destination / "prebuilds/darwin-arm64/bare-tls.bare"
    prebuild.parent.mkdir(parents=True)
    shutil.copy2(artifact, prebuild)
    (destination / "binding.js").write_text(
        "module.exports = require.addon('./prebuilds/darwin-arm64/bare-tls.bare')\n"
    )
    for dependency in (ROOT / "node_modules").iterdir():
        if dependency.name == "bare-tls":
            continue
        os.symlink(dependency, modules / dependency.name, target_is_directory=dependency.is_dir())
    script = directory / "mtls_test.mjs"
    shutil.copy2(ROOT / "bare/tests/mtls_test.mjs", script)
    return script


class BareTlsIntegrationTests(unittest.TestCase):
    @unittest.skipUnless(sys.platform == "darwin", "compiled Bare TLS test requires macOS")
    def test_mtls_handshakes(self) -> None:
        if OPENSSL is None:
            self.fail("openssl is required for the compiled Bare TLS test")
        artifact = Path(
            os.environ.get(
                "APPD_BARE_TLS_ARTIFACT",
                ROOT / "target/bare/sdk/macos-arm64/bare-tls.bare",
            )
        )
        if not artifact.is_file():
            if os.environ.get("APPD_REQUIRE_NATIVE_TLS_TEST") == "1":
                self.fail(f"compiled Bare TLS artifact is missing: {artifact}")
            self.skipTest(f"compiled Bare TLS artifact is missing: {artifact}")
        node = shutil.which("node")
        bare = ROOT / "node_modules/bare/bin/bare"
        if node is None:
            self.fail("node is required for the compiled Bare TLS test")
        if not bare.is_file():
            self.fail(f"Bare executable is missing: {bare}")
        bare_command = [node, str(bare)]
        if sys.platform == "darwin":
            bare_command = ["arch", "-arm64", *bare_command]

        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            certificates = directory / "certificates"
            certificates.mkdir()
            create_certificates(certificates)
            script = stage_runtime(directory, artifact)
            for mode in ("valid", "missing-client", "wrong-client-ca", "hostname", "expired"):
                result = subprocess.run(
                    [*bare_command, str(script), mode, str(certificates)],
                    cwd=directory,
                    capture_output=True,
                    text=True,
                    timeout=20,
                )
                self.assertEqual(
                    result.returncode,
                    0,
                    f"Bare TLS {mode} case failed\nstdout: {result.stdout}\nstderr: {result.stderr}",
                )


if __name__ == "__main__":
    unittest.main()
