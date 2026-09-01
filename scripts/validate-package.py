#!/usr/bin/env python3
"""Validate the repository Skill, Codex plugin, and release invariants."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GENERAL_SKILL = ROOT / ".agents/skills/openbaud/SKILL.md"
SCAFFOLD_SKILL = ROOT / "crates/openbaud/src/scaffold/SKILL.md"
PLATFORMS = {
    "darwin-arm64": Path("bin/darwin-arm64/openbaud"),
    "darwin-x64": Path("bin/darwin-x64/openbaud"),
    "linux-x64": Path("bin/linux-x64/openbaud"),
    "linux-arm64": Path("bin/linux-arm64/openbaud"),
    "windows-x64": Path("bin/windows-x64/openbaud.exe"),
}
UNIVERSAL = "universal"


def error(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def display_path(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return str(path)


def read_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        error(f"cannot read {display_path(path)}: {exc}")


def validate_skill(path: Path) -> None:
    try:
        text = path.read_text()
    except OSError as exc:
        error(f"cannot read {display_path(path)}: {exc}")

    match = re.match(r"\A---\n(?P<header>.*?)\n---\n", text, re.DOTALL)
    if not match:
        error(f"{display_path(path)} has no YAML frontmatter")
    header = match.group("header")
    if not re.search(r"(?m)^name:\s*openbaud\s*$", header):
        error(f"{display_path(path)} must declare name: openbaud")
    if not re.search(r"(?m)^description:\s*\S.+$", header):
        error(f"{display_path(path)} must declare a description")


def validate_plugin(package_root: Path, platform: str, version: str) -> None:
    plugin_root = package_root / "plugins/openbaud"
    plugin_skill = plugin_root / "skills/openbaud/SKILL.md"
    plugin_manifest = plugin_root / ".codex-plugin/plugin.json"
    marketplace_path = package_root / ".agents/plugins/marketplace.json"
    mcp_config = plugin_root / ".mcp.json"
    launcher = plugin_root / "launcher.mjs"

    validate_skill(plugin_skill)
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    if cargo["workspace"]["package"]["version"] != version:
        error("package validation version does not match Cargo")
    manifest = read_json(plugin_manifest)
    if manifest.get("name") != "openbaud":
        error("plugin manifest name must be openbaud")
    if manifest.get("version") != version:
        error(f"plugin version {manifest.get('version')} does not match Cargo {version}")
    if manifest.get("skills") != "./skills/":
        error("plugin skills path must remain ./skills/")

    marketplace = read_json(marketplace_path)
    if marketplace.get("name") != "openbaud-marketplace":
        error("marketplace name must be openbaud-marketplace")
    entries = [item for item in marketplace.get("plugins", []) if item.get("name") == "openbaud"]
    if len(entries) != 1:
        error("marketplace must contain exactly one openbaud entry")
    if entries[0].get("source") != {"source": "local", "path": "./plugins/openbaud"}:
        error("marketplace openbaud source must be ./plugins/openbaud")

    server = read_json(mcp_config).get("mcpServers", {}).get("openbaud", {})
    if server.get("command") != "node":
        error("plugin MCP must launch through the portable Node launcher")
    if server.get("args") != ["./launcher.mjs", "mcp"] or server.get("cwd") != ".":
        error("plugin MCP must launch launcher.mjs with args [mcp] and cwd .")
    if not launcher.is_file():
        error("plugin launcher.mjs is missing")

    selected = PLATFORMS if platform == UNIVERSAL else {platform: PLATFORMS[platform]}
    expected_files = {
        relative.as_posix()
        for relative in selected.values()
    } | {
        f"{relative.as_posix()}.sha256"
        for relative in selected.values()
    }
    bin_root = plugin_root / "bin"
    actual_files = {
        path.relative_to(plugin_root).as_posix()
        for path in bin_root.rglob("*")
        if path.is_file()
    }
    if actual_files != expected_files:
        error(
            "plugin runtime files do not match "
            f"{platform}: {sorted(actual_files ^ expected_files)}"
        )

    for relative in selected.values():
        binary = plugin_root / relative
        checksum = binary.with_name(f"{binary.name}.sha256")
        try:
            expected, filename = checksum.read_text().strip().split(maxsplit=1)
        except (OSError, ValueError) as exc:
            error(f"invalid runtime checksum file for {relative}: {exc}")
        if filename != binary.name:
            error(f"runtime checksum for {relative} must name {binary.name}")
        actual = hashlib.sha256(binary.read_bytes()).hexdigest()
        if actual != expected:
            error(f"bundled runtime checksum does not match for {relative}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", help="release tag expected to match the package version")
    parser.add_argument(
        "--package-root",
        type=Path,
        help="extracted marketplace root to validate instead of the source tree",
    )
    parser.add_argument(
        "--platform",
        choices=(*PLATFORMS, UNIVERSAL),
        help="runtime set expected under --package-root",
    )
    args = parser.parse_args()

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = cargo["workspace"]["package"]["version"]

    if args.package_root is None:
        if args.platform is not None:
            parser.error("--platform requires --package-root")
        validate_skill(GENERAL_SKILL)
        if GENERAL_SKILL.read_bytes() != SCAFFOLD_SKILL.read_bytes():
            error("the general Skill and openbaud init scaffold have drifted")
        if (ROOT / ".claude/skills/openbaud/SKILL.md").exists():
            error("the legacy .claude Skill path must not be restored")

        init_source = (ROOT / "crates/openbaud/src/cmd_init.rs").read_text()
        runtime_source = init_source.split("#[cfg(test)]", maxsplit=1)[0]
        if (
            '.agents/skills/openbaud' not in runtime_source
            or '.claude/skills/openbaud' in runtime_source
        ):
            error("openbaud init must write only to .agents/skills/openbaud")
        package_root = ROOT
        platform = "darwin-arm64"
    else:
        if args.platform is None:
            parser.error("--platform is required with --package-root")
        package_root = args.package_root.resolve()
        if not package_root.is_dir():
            parser.error(f"package root is not a directory: {package_root}")
        top_level = {path.name for path in package_root.iterdir()}
        if top_level != {".agents", "plugins"}:
            error(
                "packaged marketplace root must contain only .agents and plugins: "
                f"{sorted(top_level)}"
            )
        platform = args.platform

    validate_plugin(package_root, platform, version)

    if args.tag:
        if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
            error(f"release tag is not stable semver: {args.tag}")
        if args.tag != f"v{version}":
            error(f"release tag {args.tag} does not match package v{version}")

    print(f"OpenBaud package {version} ({platform}) is valid")


if __name__ == "__main__":
    main()
