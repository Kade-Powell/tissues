#!/usr/bin/env python3
from __future__ import annotations

import sys
from pathlib import Path


def update_manifest(version: str) -> None:
    path = Path("Cargo.toml")
    lines = path.read_text().splitlines()
    in_package = False
    updated = False

    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("["):
            break
        if in_package and stripped.startswith("version") and "=" in stripped:
            lines[idx] = f'version = "{version}"'
            updated = True
            break

    if not updated:
        raise SystemExit("version field not found in Cargo.toml [package]")

    path.write_text("\n".join(lines) + "\n")


def update_lockfile(version: str) -> None:
    path = Path("Cargo.lock")
    lines = path.read_text().splitlines()
    in_tissue = False
    updated = False

    for idx, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[[package]]":
            in_tissue = False
            continue
        if stripped == 'name = "tissues"':
            in_tissue = True
            continue
        if in_tissue and stripped.startswith("version") and "=" in stripped:
            lines[idx] = f'version = "{version}"'
            updated = True
            break

    if not updated:
        raise SystemExit("tissues package entry not found in Cargo.lock")

    path.write_text("\n".join(lines) + "\n")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: set-cargo-version.py <semver>")
    update_manifest(sys.argv[1])
    update_lockfile(sys.argv[1])


if __name__ == "__main__":
    main()
