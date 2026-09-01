from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PACKAGER = ROOT / "scripts/package-plugin.py"
PLATFORMS = {
    "darwin-arm64": Path("bin/darwin-arm64/openbaud"),
    "darwin-x64": Path("bin/darwin-x64/openbaud"),
    "linux-x64": Path("bin/linux-x64/openbaud"),
    "linux-arm64": Path("bin/linux-arm64/openbaud"),
    "windows-x64": Path("bin/windows-x64/openbaud.exe"),
}


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


class UniversalPluginPackageTest(unittest.TestCase):
    def setUp(self) -> None:
        cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
        self.tag = f"v{cargo['workspace']['package']['version']}"

    def write_platform_archive(self, output: Path, platform: str) -> None:
        binary_path = PLATFORMS[platform]
        archive_name = f"openbaud-codex-plugin-{self.tag}-{platform}.tar.gz"
        archive_path = output / archive_name
        release_root_name = f"openbaud-{self.tag}-{platform}"
        binary = f"test-runtime:{platform}".encode()

        with tempfile.TemporaryDirectory(prefix="openbaud-platform-fixture-") as temp:
            root = Path(temp) / release_root_name / "plugins/openbaud"
            staged_binary = root / binary_path
            staged_binary.parent.mkdir(parents=True)
            staged_binary.write_bytes(binary)
            staged_binary.with_name(f"{staged_binary.name}.sha256").write_text(
                f"{sha256_bytes(binary)}  {staged_binary.name}\n", encoding="utf-8"
            )
            with tarfile.open(archive_path, "w:gz") as archive:
                archive.add(Path(temp) / release_root_name, arcname=release_root_name)

        archive_path.with_name(f"{archive_name}.sha256").write_text(
            f"{sha256_bytes(archive_path.read_bytes())}  {archive_name}\n",
            encoding="utf-8",
        )

    def test_aggregates_verified_platform_archives(self) -> None:
        with tempfile.TemporaryDirectory(prefix="openbaud-universal-test-") as temp:
            output = Path(temp)
            for platform in PLATFORMS:
                self.write_platform_archive(output, platform)

            subprocess.run(
                [
                    sys.executable,
                    str(PACKAGER),
                    "--platform",
                    "universal",
                    "--tag",
                    self.tag,
                    "--input-dir",
                    str(output),
                    "--output-dir",
                    str(output),
                ],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )

            archive_name = f"openbaud-codex-plugin-{self.tag}-universal.tar.gz"
            archive_path = output / archive_name
            expected, recorded_name = archive_path.with_name(
                f"{archive_name}.sha256"
            ).read_text(encoding="utf-8").split()
            self.assertEqual(recorded_name, archive_name)
            self.assertEqual(expected, sha256_bytes(archive_path.read_bytes()))

            root = f"openbaud-{self.tag}-universal/plugins/openbaud"
            with tarfile.open(archive_path, "r:gz") as archive:
                for platform, binary_path in PLATFORMS.items():
                    member = archive.extractfile(f"{root}/{binary_path.as_posix()}")
                    self.assertIsNotNone(member)
                    self.assertEqual(member.read(), f"test-runtime:{platform}".encode())

                mcp_file = archive.extractfile(f"{root}/.mcp.json")
                self.assertIsNotNone(mcp_file)
                server = json.loads(mcp_file.read())["mcpServers"]["openbaud"]
                self.assertEqual(server["command"], "node")
                self.assertEqual(server["args"], ["./launcher.mjs", "mcp"])

            extracted = output / "extracted"
            with tarfile.open(archive_path, "r:gz") as archive:
                archive.extractall(extracted, filter="data")
            subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/validate-package.py"),
                    "--package-root",
                    str(extracted / f"openbaud-{self.tag}-universal"),
                    "--platform",
                    "universal",
                ],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            )

    def test_rejects_a_tampered_platform_archive(self) -> None:
        with tempfile.TemporaryDirectory(prefix="openbaud-universal-test-") as temp:
            output = Path(temp)
            for platform in PLATFORMS:
                self.write_platform_archive(output, platform)

            target = output / f"openbaud-codex-plugin-{self.tag}-linux-x64.tar.gz"
            target.write_bytes(target.read_bytes() + b"tampered")
            result = subprocess.run(
                [
                    sys.executable,
                    str(PACKAGER),
                    "--platform",
                    "universal",
                    "--tag",
                    self.tag,
                    "--input-dir",
                    str(output),
                    "--output-dir",
                    str(output),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("release archive checksum mismatch", result.stderr)


if __name__ == "__main__":
    unittest.main()
