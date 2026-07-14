#!/usr/bin/env python3
"""Build a Codex++ package with a temporary fork version."""

import argparse
import os
import re
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
CODEX_RS = REPO_ROOT / "codex-rs"
CARGO_TOML = CODEX_RS / "Cargo.toml"
CARGO_LOCK = CODEX_RS / "Cargo.lock"
DEFAULT_PACKAGE_DIR = "dist/codex-plus-plus"
WORKSPACE_VERSION_PATTERN = re.compile(r'^(version\s*=\s*")[^"]+(")', re.MULTILINE)

sys.path.insert(0, str(REPO_ROOT / "scripts"))

from codex_package.cli import parse_args as parse_package_args  # noqa: E402
from codex_package.version import read_workspace_version  # noqa: E402


def main() -> int:
    args, package_args = parse_args()
    base_version = read_workspace_version()
    fork_version = args.fork_version or suffixed_version(base_version, args.suffix)
    tag_name = f"{args.tag_prefix}{fork_version}"
    package_args = default_package_args(package_args)

    original_toml = CARGO_TOML.read_text(encoding="utf-8")
    original_lock = CARGO_LOCK.read_bytes() if CARGO_LOCK.exists() else None
    CARGO_TOML.write_text(
        replace_workspace_version(original_toml, fork_version),
        encoding="utf-8",
        newline="",
    )
    try:
        print(f"Codex++ package version: {fork_version}", flush=True)
        print(f"Suggested git tag: {tag_name}", flush=True)
        build_status = subprocess.call(
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

    if build_status != 0 or not args.install:
        return build_status
    return install_package(package_args)


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
        "--install",
        action="store_true",
        help="Install the completed native package as the active Codex++ command.",
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
    defaults: list[str] = []
    if option_value(package_args, "--cargo-profile") is None:
        defaults.extend(["--cargo-profile", "release-fast"])
    if package_dir_arg(package_args) is None:
        defaults.extend(["--package-dir", DEFAULT_PACKAGE_DIR, "--force"])
    return [*defaults, *package_args]


def option_value(args: list[str], option: str) -> str | None:
    for index in range(len(args) - 1, -1, -1):
        arg = args[index]
        if arg == option:
            return args[index + 1] if index + 1 < len(args) else None
        prefix = f"{option}="
        if arg.startswith(prefix):
            return arg[len(prefix) :]
    return None


def package_dir_arg(args: list[str]) -> Path | None:
    return getattr(parse_package_args(args), "package_dir", None)


def install_package(package_args: list[str]) -> int:
    package_dir = package_dir_arg(package_args)
    if package_dir is None:
        raise RuntimeError("Codex++ package directory was not configured")
    if not package_dir.is_absolute():
        package_dir = REPO_ROOT / package_dir
    target_exe = package_dir / "bin" / ("codex.exe" if os.name == "nt" else "codex")
    if not target_exe.is_file():
        print(f"Built Codex++ executable was not found: {target_exe}", file=sys.stderr)
        return 1

    if os.name == "nt":
        command = [
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(REPO_ROOT / "scripts" / "install" / "install-codex-plus-plus.ps1"),
            "-TargetExe",
            str(target_exe),
            "-Install",
            "-AddToUserPath",
        ]
    else:
        command = [
            "sh",
            str(REPO_ROOT / "scripts" / "install" / "install-codex-plus-plus.sh"),
            "--target-exe",
            str(target_exe),
            "--install",
        ]
    return subprocess.call(command, cwd=REPO_ROOT)


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
