#!/usr/bin/env python3
"""Create a self-contained, platform-specific OpenBaud Codex plugin archive."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import tarfile
import tempfile
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PLUGIN_ROOT = ROOT / "plugins/openbaud"
MARKETPLACE = ROOT / ".agents/plugins/marketplace.json"
PLATFORMS = {
    "darwin-arm64": Path("bin/darwin-arm64/openbaud"),
    "darwin-x64": Path("bin/darwin-x64/openbaud"),
    "linux-x64": Path("bin/linux-x64/openbaud"),
    "linux-arm64": Path("bin/linux-arm64/openbaud"),
    "windows-x64": Path("bin/windows-x64/openbaud.exe"),
}


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def normalized_tarinfo(info: tarfile.TarInfo) -> tarfile.TarInfo:
    info.uid = 0
    info.gid = 0
    info.uname = "root"
    info.gname = "root"
    info.mtime = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
    return info


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=PLATFORMS, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    args = parser.parse_args()

    if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
        parser.error(f"tag is not stable semver: {args.tag}")

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = cargo["workspace"]["package"]["version"]
    if args.tag != f"v{version}":
        parser.error(f"tag {args.tag} does not match package v{version}")

    binary_path = PLATFORMS[args.platform]
    source_binary = PLUGIN_ROOT / binary_path
    if not source_binary.is_file():
        parser.error(f"bundled runtime is missing: {source_binary.relative_to(ROOT)}")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    archive_name = f"openbaud-codex-plugin-{args.tag}-{args.platform}.tar.gz"
    archive_path = args.output_dir / archive_name
    release_root_name = f"openbaud-{args.tag}-{args.platform}"

    with tempfile.TemporaryDirectory(prefix="openbaud-package-") as temp:
        release_root = Path(temp) / release_root_name
        stage = release_root / "plugins/openbaud"
        shutil.copytree(PLUGIN_ROOT, stage)
        marketplace = release_root / ".agents/plugins/marketplace.json"
        marketplace.parent.mkdir(parents=True)
        shutil.copy2(MARKETPLACE, marketplace)

        shutil.rmtree(stage / "bin")
        staged_binary = stage / binary_path
        staged_binary.parent.mkdir(parents=True)
        shutil.copy2(source_binary, staged_binary)

        checksum_path = staged_binary.with_name(f"{staged_binary.name}.sha256")
        checksum_path.write_text(f"{digest(staged_binary)}  {staged_binary.name}\n")

        mcp_path = stage / ".mcp.json"
        mcp = json.loads(mcp_path.read_text())
        mcp["mcpServers"]["openbaud"]["command"] = f"./{binary_path.as_posix()}"
        mcp_path.write_text(json.dumps(mcp, indent=2) + "\n")

        with tarfile.open(archive_path, "w:gz", compresslevel=9) as archive:
            archive.add(
                release_root,
                arcname=release_root_name,
                filter=normalized_tarinfo,
            )

    checksum = digest(archive_path)
    archive_path.with_name(f"{archive_name}.sha256").write_text(
        f"{checksum}  {archive_name}\n"
    )
    print(archive_path)


if __name__ == "__main__":
    main()
