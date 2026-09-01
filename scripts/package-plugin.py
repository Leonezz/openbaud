#!/usr/bin/env python3
"""Create platform-specific or universal OpenBaud Codex plugin archives."""

from __future__ import annotations

import argparse
import hashlib
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
UNIVERSAL = "universal"


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


def read_checksum(path: Path, expected_filename: str) -> str:
    try:
        expected, filename = path.read_text(encoding="utf-8").strip().split()
    except (OSError, ValueError) as exc:
        raise ValueError(f"invalid checksum file {path}: {exc}") from exc
    if filename != expected_filename:
        raise ValueError(
            f"checksum {path} names {filename!r}, expected {expected_filename!r}"
        )
    if not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError(f"checksum {path} is not a SHA-256 digest")
    return expected


def archive_member_bytes(archive: tarfile.TarFile, name: str) -> bytes:
    try:
        member = archive.getmember(name)
    except KeyError as exc:
        raise ValueError(f"archive is missing {name}") from exc
    if not member.isfile():
        raise ValueError(f"archive member is not a regular file: {name}")
    stream = archive.extractfile(member)
    if stream is None:
        raise ValueError(f"archive member cannot be read: {name}")
    return stream.read()


def copy_runtime_from_archive(
    *, platform: str, tag: str, input_dir: Path, destination: Path
) -> None:
    archive_name = f"openbaud-codex-plugin-{tag}-{platform}.tar.gz"
    archive_path = input_dir / archive_name
    archive_checksum = read_checksum(
        archive_path.with_name(f"{archive_name}.sha256"), archive_name
    )
    if digest(archive_path) != archive_checksum:
        raise ValueError(f"release archive checksum mismatch: {archive_name}")

    binary_path = PLATFORMS[platform]
    root = f"openbaud-{tag}-{platform}/plugins/openbaud"
    binary_member = f"{root}/{binary_path.as_posix()}"
    checksum_member = f"{binary_member}.sha256"
    with tarfile.open(archive_path, "r:gz") as archive:
        binary = archive_member_bytes(archive, binary_member)
        checksum_text = archive_member_bytes(archive, checksum_member).decode("utf-8")

    try:
        expected, filename = checksum_text.strip().split()
    except ValueError as exc:
        raise ValueError(f"invalid runtime checksum in {archive_name}") from exc
    if filename != binary_path.name:
        raise ValueError(
            f"runtime checksum in {archive_name} names {filename!r}, "
            f"expected {binary_path.name!r}"
        )
    actual = hashlib.sha256(binary).hexdigest()
    if actual != expected:
        raise ValueError(f"runtime checksum mismatch in {archive_name}")

    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(binary)
    destination.chmod(0o755)
    destination.with_name(f"{destination.name}.sha256").write_text(
        f"{actual}  {destination.name}\n", encoding="utf-8"
    )


def copy_local_runtime(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)
    destination.with_name(f"{destination.name}.sha256").write_text(
        f"{digest(destination)}  {destination.name}\n", encoding="utf-8"
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--platform", choices=(*PLATFORMS, UNIVERSAL), required=True
    )
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "dist")
    parser.add_argument(
        "--input-dir",
        type=Path,
        help="directory containing all platform archives when building universal",
    )
    args = parser.parse_args()

    if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
        parser.error(f"tag is not stable semver: {args.tag}")

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = cargo["workspace"]["package"]["version"]
    if args.tag != f"v{version}":
        parser.error(f"tag {args.tag} does not match package v{version}")

    if args.platform == UNIVERSAL:
        if args.input_dir is None:
            parser.error("--input-dir is required when --platform universal")
    else:
        binary_path = PLATFORMS[args.platform]
        source_binary = PLUGIN_ROOT / binary_path
        if not source_binary.is_file():
            parser.error(
                f"bundled runtime is missing: {source_binary.relative_to(ROOT)}"
            )

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
        if args.platform == UNIVERSAL:
            try:
                for platform, runtime_path in PLATFORMS.items():
                    copy_runtime_from_archive(
                        platform=platform,
                        tag=args.tag,
                        input_dir=args.input_dir,
                        destination=stage / runtime_path,
                    )
            except (OSError, ValueError, tarfile.TarError) as exc:
                parser.error(str(exc))
        else:
            copy_local_runtime(source_binary, stage / binary_path)

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
