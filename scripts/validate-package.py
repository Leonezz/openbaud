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
PLUGIN_ROOT = ROOT / "plugins/openbaud"
PLUGIN_SKILL = PLUGIN_ROOT / "skills/openbaud/SKILL.md"
PLUGIN_MANIFEST = PLUGIN_ROOT / ".codex-plugin/plugin.json"
MARKETPLACE = ROOT / ".agents/plugins/marketplace.json"
MCP_CONFIG = PLUGIN_ROOT / ".mcp.json"
BINARY = PLUGIN_ROOT / "bin/darwin-arm64/openbaud"
CHECKSUM = BINARY.with_name("openbaud.sha256")


def error(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def read_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        error(f"cannot read {path.relative_to(ROOT)}: {exc}")


def validate_skill(path: Path) -> None:
    try:
        text = path.read_text()
    except OSError as exc:
        error(f"cannot read {path.relative_to(ROOT)}: {exc}")

    match = re.match(r"\A---\n(?P<header>.*?)\n---\n", text, re.DOTALL)
    if not match:
        error(f"{path.relative_to(ROOT)} has no YAML frontmatter")
    header = match.group("header")
    if not re.search(r"(?m)^name:\s*openbaud\s*$", header):
        error(f"{path.relative_to(ROOT)} must declare name: openbaud")
    if not re.search(r"(?m)^description:\s*\S.+$", header):
        error(f"{path.relative_to(ROOT)} must declare a description")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", help="release tag expected to match the package version")
    args = parser.parse_args()

    validate_skill(GENERAL_SKILL)
    validate_skill(PLUGIN_SKILL)
    if GENERAL_SKILL.read_bytes() != SCAFFOLD_SKILL.read_bytes():
        error("the general Skill and openbaud init scaffold have drifted")
    if (ROOT / ".claude/skills/openbaud/SKILL.md").exists():
        error("the legacy .claude Skill path must not be restored")

    init_source = (ROOT / "crates/openbaud/src/cmd_init.rs").read_text()
    runtime_source = init_source.split("#[cfg(test)]", maxsplit=1)[0]
    if '.agents/skills/openbaud' not in runtime_source or '.claude/skills/openbaud' in runtime_source:
        error("openbaud init must write only to .agents/skills/openbaud")

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text())
    version = cargo["workspace"]["package"]["version"]
    manifest = read_json(PLUGIN_MANIFEST)
    if manifest.get("name") != "openbaud":
        error("plugin manifest name must be openbaud")
    if manifest.get("version") != version:
        error(f"plugin version {manifest.get('version')} does not match Cargo {version}")
    if manifest.get("skills") != "./skills/":
        error("plugin skills path must remain ./skills/")

    marketplace = read_json(MARKETPLACE)
    if marketplace.get("name") != "openbaud-marketplace":
        error("marketplace name must be openbaud-marketplace")
    entries = [item for item in marketplace.get("plugins", []) if item.get("name") == "openbaud"]
    if len(entries) != 1:
        error("marketplace must contain exactly one openbaud entry")
    if entries[0].get("source") != {"source": "local", "path": "./plugins/openbaud"}:
        error("marketplace openbaud source must be ./plugins/openbaud")

    server = read_json(MCP_CONFIG).get("mcpServers", {}).get("openbaud", {})
    if server.get("command") != "./bin/darwin-arm64/openbaud":
        error("plugin MCP must launch the bundled darwin-arm64 runtime")
    if server.get("args") != ["mcp"] or server.get("cwd") != ".":
        error("plugin MCP must launch with args [mcp] and cwd .")

    if not BINARY.is_file():
        error("bundled darwin-arm64 runtime is missing")
    try:
        expected, filename = CHECKSUM.read_text().strip().split(maxsplit=1)
    except (OSError, ValueError) as exc:
        error(f"invalid runtime checksum file: {exc}")
    if filename != "openbaud":
        error("runtime checksum must name openbaud")
    actual = hashlib.sha256(BINARY.read_bytes()).hexdigest()
    if actual != expected:
        error("bundled runtime checksum does not match")

    if args.tag:
        if not re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
            error(f"release tag is not stable semver: {args.tag}")
        if args.tag != f"v{version}":
            error(f"release tag {args.tag} does not match package v{version}")

    print(f"OpenBaud package {version} is valid")


if __name__ == "__main__":
    main()
