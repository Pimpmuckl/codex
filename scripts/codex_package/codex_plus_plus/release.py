#!/usr/bin/env python3
"""Hydrate, verify, and publish recoverable Codex++ npm artifacts."""

import argparse
import base64
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import time
import zipfile


PACKAGE_NAME = "@jjliebig/codex-plus-plus"
REPOSITORY_URL = "git+https://github.com/Pimpmuckl/codex.git"


@dataclass(frozen=True)
class Platform:
    tag: str
    target: str
    archive_suffix: str
    binary: str


PLATFORMS = (
    Platform("linux-x64", "x86_64-unknown-linux-musl", "tar.gz", "codex"),
    Platform("darwin-arm64", "aarch64-apple-darwin", "tar.gz", "codex"),
    Platform("win32-x64", "x86_64-pc-windows-msvc", "zip", "codex.exe"),
)


def archive_path(directory: Path, version: str, platform: Platform) -> Path:
    return directory / (
        f"codex-plus-plus-{version}-{platform.target}.{platform.archive_suffix}"
    )


def npm_tarball_path(directory: Path, version: str, tag: str | None) -> Path:
    suffix = f"-{tag}" if tag else ""
    return directory / f"codex-plus-plus-npm{suffix}-{version}.tgz"


def hydrate(version: str, archives_dir: Path, vendor_dir: Path) -> None:
    vendor_dir.mkdir(parents=True, exist_ok=True)
    for platform in PLATFORMS:
        destination = vendor_dir / platform.target
        if destination.exists():
            raise RuntimeError(f"Vendor target already exists: {destination}")
        source = archive_path(archives_dir, version, platform)
        if not source.is_file():
            raise RuntimeError(f"Missing release archive: {source}")
        shutil.unpack_archive(source, destination)
        for relative in ("codex-package.json", f"bin/{platform.binary}"):
            if not (destination / relative).is_file():
                raise RuntimeError(f"Archive {source.name} is missing {relative}")


def verify(version: str, archives_dir: Path, npm_dir: Path) -> None:
    for platform in PLATFORMS:
        archive = archive_path(archives_dir, version, platform)
        tarball = npm_tarball_path(npm_dir, version, platform.tag)
        archive_files = read_release_files(archive)
        npm_files = read_npm_files(tarball, platform)
        if archive_files != npm_files:
            raise RuntimeError(
                f"Native payload mismatch for {platform.target}: "
                f"{digest(archive_files)} != {digest(npm_files)}"
            )
        print(f"{platform.target}: {digest(archive_files)}", flush=True)


def read_release_files(path: Path) -> dict[str, bytes]:
    if path.suffix == ".zip":
        with zipfile.ZipFile(path) as archive:
            return {
                member.filename: archive.read(member)
                for member in archive.infolist()
                if not member.is_dir()
            }
    with tarfile.open(path, "r:gz") as archive:
        return {
            member.name: archive.extractfile(member).read()
            for member in archive.getmembers()
            if member.isfile()
        }


def read_npm_files(path: Path, platform: Platform) -> dict[str, bytes]:
    prefix = f"package/vendor/{platform.target}/"
    with tarfile.open(path, "r:gz") as archive:
        return {
            member.name.removeprefix(prefix): archive.extractfile(member).read()
            for member in archive.getmembers()
            if member.isfile() and member.name.startswith(prefix)
        }


def digest(files: dict[str, bytes]) -> str:
    payload = b"".join(name.encode() + b"\0" + files[name] for name in sorted(files))
    return hashlib.sha256(payload).hexdigest()


def tarball_integrity(path: Path) -> str:
    return (
        "sha512-"
        + base64.b64encode(hashlib.sha512(path.read_bytes()).digest()).decode()
    )


def read_manifest(path: Path) -> dict:
    with tarfile.open(path, "r:gz") as archive:
        manifest_file = archive.extractfile("package/package.json")
        if manifest_file is None:
            raise RuntimeError(f"Missing package/package.json in {path}")
        return json.load(manifest_file)


def release_entries(version: str, npm_dir: Path) -> list[tuple[Path, str, str]]:
    entries = [
        (
            npm_tarball_path(npm_dir, version, platform.tag),
            f"{version}-{platform.tag}",
            platform.tag,
        )
        for platform in PLATFORMS
    ]
    entries.append((npm_tarball_path(npm_dir, version, None), version, "latest"))
    return entries


def validate_tarballs(version: str, npm_dir: Path) -> list[tuple[Path, str, str]]:
    entries = release_entries(version, npm_dir)
    for path, expected_version, _tag in entries:
        if not path.is_file():
            raise RuntimeError(f"Missing npm tarball: {path}")
        manifest = read_manifest(path)
        expected_repository = {
            "type": "git",
            "url": REPOSITORY_URL,
            "directory": "codex-cli",
        }
        if (
            manifest.get("name"),
            manifest.get("version"),
            manifest.get("repository"),
        ) != (
            PACKAGE_NAME,
            expected_version,
            expected_repository,
        ):
            raise RuntimeError(f"Unexpected npm manifest in {path}: {manifest}")
    return entries


def npm_view(spec: str, field: str) -> str | None:
    result = subprocess.run(
        ["npm", "view", spec, field, "--json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode == 0:
        value = json.loads(result.stdout)
        return value if isinstance(value, str) else None
    if "E404" in result.stderr:
        return None
    raise RuntimeError(result.stderr.strip() or f"npm view failed for {spec}")


def publish(version: str, npm_dir: Path, *, dry_run: bool = False) -> None:
    entries = validate_tarballs(version, npm_dir)
    bootstrap_specs = (PACKAGE_NAME, f"{PACKAGE_NAME}@{entries[0][1]}")
    if all(npm_view(spec, "name") is None for spec in bootstrap_specs):
        raise RuntimeError(
            f"{PACKAGE_NAME} does not exist. Download this run's private npm artifact, "
            "manually publish its linux-x64 tarball under --tag linux-x64, configure "
            "trusted publishing for Pimpmuckl/codex and codex-plus-plus-release.yml, "
            "then rerun only this failed job."
        )

    pending = []
    for path, package_version, tag in entries:
        expected = tarball_integrity(path)
        spec = f"{PACKAGE_NAME}@{package_version}"
        current = npm_view(spec, "dist.integrity")
        if current == expected:
            print(f"Skipping {spec}; registry integrity matches", flush=True)
            continue
        if current is not None:
            raise RuntimeError(
                f"Refusing to skip {spec}: registry integrity {current} != {expected}"
            )
        pending.append((path, spec, tag, expected))

    for path, spec, tag, expected in pending:
        if dry_run:
            print(f"Would publish {spec} with tag {tag}", flush=True)
            continue
        subprocess.run(
            [
                "npm",
                "publish",
                str(path),
                "--access",
                "public",
                "--tag",
                tag,
                "--provenance",
            ],
            check=True,
        )
        for attempt in range(12):
            current = npm_view(spec, "dist.integrity")
            if current == expected:
                break
            if current is not None:
                raise RuntimeError(
                    f"Published {spec} has registry integrity {current} != {expected}"
                )
            if attempt < 11:
                time.sleep(5)
        else:
            raise RuntimeError(f"Timed out confirming {spec} in the npm registry")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for command in ("hydrate", "verify"):
        subparser = subparsers.add_parser(command)
        subparser.add_argument("--version", required=True)
        subparser.add_argument("--archives-dir", type=Path, required=True)
        subparser.add_argument(
            "--vendor-dir" if command == "hydrate" else "--npm-dir",
            type=Path,
            required=True,
        )
    publish_parser = subparsers.add_parser("publish")
    publish_parser.add_argument("--version", required=True)
    publish_parser.add_argument("--npm-dir", type=Path, required=True)
    publish_parser.add_argument("--dry-run", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.command == "hydrate":
        hydrate(args.version, args.archives_dir, args.vendor_dir)
    elif args.command == "verify":
        verify(args.version, args.archives_dir, args.npm_dir)
    else:
        publish(args.version, args.npm_dir, dry_run=args.dry_run)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
