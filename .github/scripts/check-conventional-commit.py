#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


ALLOWED_TYPES = {
    "build",
    "chore",
    "ci",
    "docs",
    "feat",
    "fix",
    "perf",
    "refactor",
    "revert",
    "style",
    "test",
}
HEADER_RE = re.compile(
    r"^(?P<type>[a-z]+)(?:\([a-z0-9._/-]+\))?(?P<breaking>!)?: (?P<description>\S.+)$"
)


def first_meaningful_line(message: str) -> str:
    for line in message.splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            return stripped
    return ""


def validate(message_path: Path) -> list[str]:
    subject = first_meaningful_line(message_path.read_text())
    errors: list[str] = []

    if not subject:
        return ["commit message is empty"]

    match = HEADER_RE.match(subject)
    if not match:
        return [
            "commit subject must match: type(optional-scope): description",
            "examples: feat: add issue filters | fix(tui): wrap comments | ci: add release checks",
        ]

    commit_type = match.group("type")
    if commit_type not in ALLOWED_TYPES:
        errors.append(
            f"commit type must be one of: {', '.join(sorted(ALLOWED_TYPES))}"
        )

    description = match.group("description")
    if description.endswith("."):
        errors.append("commit description must not end with a period")

    return errors


def main() -> int:
    if len(sys.argv) < 2:
        print("usage: check-conventional-commit.py <commit-message-file> [...]", file=sys.stderr)
        return 2

    failed = False
    for raw_path in sys.argv[1:]:
        path = Path(raw_path)
        errors = validate(path)
        if errors:
            failed = True
            print(f"{path}: invalid Conventional Commit message", file=sys.stderr)
            for error in errors:
                print(f"  - {error}", file=sys.stderr)

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
