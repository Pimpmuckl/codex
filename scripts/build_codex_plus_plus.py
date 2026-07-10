#!/usr/bin/env python3
"""Build a Codex++ package with a temporary fork version."""

import argparse
import re
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
CODEX_RS = REPO_ROOT / "codex-rs"
CARGO_TOML = CODEX_RS / "Cargo.toml"
CARGO_LOCK = CODEX_RS / "Cargo.lock"
WORKSPACE_VERSION_PATTERN = re.compile(r'^(version\s*=\s*")[^"]+(")', re.MULTILINE)

sys.path.insert(0, str(REPO_ROOT / "scripts"))

from codex_package.version import read_workspace_version  # noqa: E402
from codex_package.targets import TARGET_SPECS  # noqa: E402
from codex_package.targets import default_target  # noqa: E402


MIGRATION_FILES = tuple(sorted((CODEX_RS / "state").glob("*migrations/*.sql")))


def main() -> int:
    args, package_args = parse_args()
    base_version = read_workspace_version()
    fork_version = args.fork_version or suffixed_version(base_version, args.suffix)
    tag_name = f"{args.tag_prefix}{fork_version}"
    package_args = default_package_args(package_args)

    original_toml = CARGO_TOML.read_text(encoding="utf-8")
    original_lock = CARGO_LOCK.read_bytes() if CARGO_LOCK.exists() else None
    original_migrations = (
        {path: path.read_bytes() for path in MIGRATION_FILES}
        if builds_windows_package(package_args)
        else {}
    )
    CARGO_TOML.write_text(
        replace_workspace_version(original_toml, fork_version),
        encoding="utf-8",
        newline="",
    )
    try:
        for path, contents in original_migrations.items():
            path.write_bytes(with_crlf_line_endings(contents))
        print(f"Codex++ package version: {fork_version}", flush=True)
        print(f"Suggested git tag: {tag_name}", flush=True)
        return subprocess.call(
            [
                sys.executable,
                str(REPO_ROOT / "scripts" / "build_codex_package.py"),
                *package_args,
            ],
            cwd=REPO_ROOT,
        )
    finally:
        CARGO_TOML.write_text(original_toml, encoding="utf-8", newline="")
        if original_lock is None:
            CARGO_LOCK.unlink(missing_ok=True)
        else:
            CARGO_LOCK.write_bytes(original_lock)
        for path, contents in original_migrations.items():
            path.write_bytes(contents)


def parse_args() -> tuple[argparse.Namespace, list[str]]:
    parser = argparse.ArgumentParser(
        description="Build Codex++ while temporarily suffixing the package version.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument(
        "--fork-version",
        help="Exact temporary package version to write into codex-rs/Cargo.toml.",
    )
    parser.add_argument(
        "--suffix",
        default="fork",
        help="Suffix appended to the upstream workspace version when --fork-version is omitted.",
    )
    parser.add_argument(
        "--tag-prefix",
        default="rust-v",
        help="Prefix used when printing the suggested fork tag.",
    )
    parser.add_argument(
        "package_args",
        nargs=argparse.REMAINDER,
        help="Arguments forwarded to scripts/build_codex_package.py. Use -- before these.",
    )
    args = parser.parse_args()
    package_args = args.package_args
    if package_args[:1] == ["--"]:
        package_args = package_args[1:]
    return args, package_args


def suffixed_version(version: str, suffix: str) -> str:
    suffix = suffix.strip().strip("-")
    if not suffix:
        return version
    return f"{version}-{suffix}"


def default_package_args(package_args: list[str]) -> list[str]:
    if "--cargo-profile" in package_args:
        return package_args
    return ["--cargo-profile", "release", *package_args]


def builds_windows_package(package_args: list[str]) -> bool:
    target = None
    for index, arg in enumerate(package_args):
        if arg == "--target":
            if index + 1 == len(package_args):
                raise ValueError("--target requires a value")
            target = package_args[index + 1]
            break
        if arg.startswith("--target="):
            target = arg.partition("=")[2]
            break
    return TARGET_SPECS[target or default_target()].is_windows


def with_crlf_line_endings(contents: bytes) -> bytes:
    return (
        contents.replace(b"\r\n", b"\n").replace(b"\r", b"\n").replace(b"\n", b"\r\n")
    )


def replace_workspace_version(cargo_toml: str, version: str) -> str:
    in_workspace_package = False
    lines = cargo_toml.splitlines(keepends=True)
    for index, line in enumerate(lines):
        stripped = line.strip()
        if stripped == "[workspace.package]":
            in_workspace_package = True
            continue
        if in_workspace_package and stripped.startswith("["):
            break
        if in_workspace_package and WORKSPACE_VERSION_PATTERN.match(stripped):
            lines[index] = WORKSPACE_VERSION_PATTERN.sub(
                rf"\g<1>{version}\2", line, count=1
            )
            return "".join(lines)
    raise RuntimeError(f"Could not find [workspace.package].version in {CARGO_TOML}")


if __name__ == "__main__":
    raise SystemExit(main())
