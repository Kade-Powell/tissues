#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import subprocess
from pathlib import Path


HEADER_RE = re.compile(r"^(?P<type>[a-z][a-z0-9-]*)(?:\([^)]+\))?(?P<bang>!)?: .+")
TAG_RE = re.compile(r"^v(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)(?:[-+].*)?$")


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], text=True).rstrip("\n")


def cargo_version() -> str:
    in_package = False
    for line in Path("Cargo.toml").read_text().splitlines():
        stripped = line.strip()
        if stripped == "[package]":
            in_package = True
            continue
        if in_package and stripped.startswith("["):
            break
        if in_package and stripped.startswith("version") and "=" in stripped:
            return stripped.split("=", 1)[1].strip().strip('"')
    raise SystemExit("version field not found in Cargo.toml [package]")


def latest_version_tag() -> str | None:
    tags = git("tag", "--list", "v[0-9]*", "--sort=-v:refname").splitlines()
    for tag in tags:
        if TAG_RE.match(tag):
            return tag
    return None


def commits_since(tag: str | None) -> list[dict[str, str]]:
    range_arg = f"{tag}..HEAD" if tag else "HEAD"
    raw = git("log", "--format=%H%x1f%s%x1f%b%x1e", range_arg)
    commits: list[dict[str, str]] = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record:
            continue
        fields = record.split("\x1f")
        if len(fields) < 3:
            continue
        commits.append({"sha": fields[0], "subject": fields[1], "body": fields[2]})
    return commits


def commit_bump(commit: dict[str, str]) -> str | None:
    match = HEADER_RE.match(commit["subject"])
    if not match:
        return None
    if match.group("bang") or re.search(r"^BREAKING[- ]CHANGE:", commit["body"], re.MULTILINE):
        return "major"
    if match.group("type") == "feat":
        return "minor"
    return "patch"


def next_version(base: str, bump: str) -> str:
    match = TAG_RE.match(f"v{base}")
    if not match:
        raise SystemExit(f"base version is not semver: {base}")
    major = int(match.group("major"))
    minor = int(match.group("minor"))
    patch = int(match.group("patch"))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    if bump == "initial":
        return base
    raise SystemExit(f"unknown bump: {bump}")


def write_output(values: dict[str, str]) -> None:
    output_path = os.environ.get("GITHUB_OUTPUT")
    if not output_path:
        return
    with open(output_path, "a", encoding="utf-8") as output:
        for key, value in values.items():
            print(f"{key}={value}", file=output)


def write_notes(tag: str, commits: list[dict[str, str]]) -> None:
    lines = [f"# {tag}", "", "## Changes", ""]
    if commits:
        for commit in commits:
            lines.append(f"- {commit['subject']} ({commit['sha'][:7]})")
    else:
        lines.append(f"- Manual release for {tag}")
    Path("release-notes.md").write_text("\n".join(lines) + "\n")


def main() -> None:
    subprocess.check_call(["git", "fetch", "--tags", "--force"], stdout=subprocess.DEVNULL)

    tag_override = os.environ.get("VERSION_TAG_OVERRIDE", "").strip()
    if tag_override:
        tag = tag_override if tag_override.startswith("v") else f"v{tag_override}"
        if not TAG_RE.match(tag):
            raise SystemExit(f"version tag override is not semver: {tag_override}")
        subprocess.check_call(["git", "rev-parse", "--verify", f"refs/tags/{tag}"])
        write_notes(tag, [])
        write_output(
            {
                "should_release": "true",
                "latest_tag": tag,
                "bump": "manual",
                "version": tag.removeprefix("v"),
                "tag": tag,
                "tag_exists": "true",
            }
        )
        print(f"Planned manual release for existing tag {tag}.")
        return

    latest_tag = latest_version_tag()
    commits = commits_since(latest_tag)
    bumps = [bump for commit in commits if (bump := commit_bump(commit))]

    if not bumps:
        print("No Conventional Commit release entries found since latest tag.")
        write_output({"should_release": "false", "tag_exists": "false"})
        return

    if latest_tag is None:
        bump = "initial"
        version = cargo_version()
    else:
        bump = "major" if "major" in bumps else "minor" if "minor" in bumps else "patch"
        version = next_version(latest_tag.removeprefix("v"), bump)

    tag = f"v{version}"
    write_notes(tag, commits)
    write_output(
        {
            "should_release": "true",
            "latest_tag": latest_tag or "",
            "bump": bump,
            "version": version,
            "tag": tag,
            "tag_exists": "false",
        }
    )
    print(f"Planned {tag} ({bump}) from {len(commits)} commit(s).")


if __name__ == "__main__":
    main()
