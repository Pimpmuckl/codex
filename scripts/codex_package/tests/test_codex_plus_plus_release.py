#!/usr/bin/env python3

import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import zipfile


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "codex_plus_plus"))

import release  # noqa: E402


VERSION = "0.144.0-fork.1"


def add_tar_file(archive: tarfile.TarFile, name: str, payload: bytes) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(payload)
    archive.addfile(info, io.BytesIO(payload))


def write_tarball(path: Path, files: dict[str, bytes]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, payload in files.items():
            add_tar_file(archive, name, payload)


def manifest(version: str) -> bytes:
    return json.dumps(
        {
            "name": release.PACKAGE_NAME,
            "version": version,
            "repository": {
                "type": "git",
                "url": release.REPOSITORY_URL,
                "directory": "codex-cli",
            },
        }
    ).encode()


def write_npm_tarballs(directory: Path) -> None:
    for path, package_version, _tag in release.release_entries(VERSION, directory):
        write_tarball(path, {"package/package.json": manifest(package_version)})


class CodexPlusPlusReleaseTest(unittest.TestCase):
    def test_verify_accepts_exact_native_payloads(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            archives = root / "archives"
            npm = root / "npm"
            archives.mkdir()
            npm.mkdir()

            for platform in release.PLATFORMS:
                payload = {
                    "codex-package.json": b'{"layoutVersion":1}',
                    f"bin/{platform.binary}": platform.target.encode(),
                }
                archive = release.archive_path(archives, VERSION, platform)
                if platform.archive_suffix == "zip":
                    with zipfile.ZipFile(archive, "w") as output:
                        for name, content in payload.items():
                            output.writestr(name, content)
                else:
                    write_tarball(archive, payload)
                write_tarball(
                    release.npm_tarball_path(npm, VERSION, platform.tag),
                    {
                        f"package/vendor/{platform.target}/{name}": content
                        for name, content in payload.items()
                    },
                )

            release.verify(VERSION, archives, npm)

    def test_publish_skips_exact_registry_versions(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            npm_dir = Path(temp)
            write_npm_tarballs(npm_dir)
            integrities = {
                f"{release.PACKAGE_NAME}@{package_version}": release.tarball_integrity(
                    path
                )
                for path, package_version, _tag in release.release_entries(
                    VERSION, npm_dir
                )
            }

            def npm_view(spec: str, field: str) -> str | None:
                if field == "name":
                    return release.PACKAGE_NAME
                return integrities[spec]

            with (
                patch.object(release, "npm_view", side_effect=npm_view),
                patch.object(release.subprocess, "run") as run,
            ):
                release.publish(VERSION, npm_dir)

            run.assert_not_called()

    def test_publish_preflights_conflicts_before_publishing(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            npm_dir = Path(temp)
            write_npm_tarballs(npm_dir)
            conflict_spec = (
                f"{release.PACKAGE_NAME}@{VERSION}-{release.PLATFORMS[1].tag}"
            )

            def npm_view(spec: str, field: str) -> str | None:
                if field == "name":
                    return release.PACKAGE_NAME
                return "sha512-conflict" if spec == conflict_spec else None

            with (
                patch.object(release, "npm_view", side_effect=npm_view),
                patch.object(release.subprocess, "run") as run,
                self.assertRaisesRegex(RuntimeError, "Refusing to skip"),
            ):
                release.publish(VERSION, npm_dir)

            run.assert_not_called()

    def test_publish_dry_run_never_invokes_npm_publish(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            npm_dir = Path(temp)
            write_npm_tarballs(npm_dir)

            def npm_view(_spec: str, field: str) -> str | None:
                return release.PACKAGE_NAME if field == "name" else None

            with (
                patch.object(release, "npm_view", side_effect=npm_view),
                patch.object(release.subprocess, "run") as run,
            ):
                release.publish(VERSION, npm_dir, dry_run=True)

            run.assert_not_called()

    def test_publish_uses_oidc_provenance_and_confirms_integrity(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            npm_dir = Path(temp)
            write_npm_tarballs(npm_dir)
            integrities = {
                f"{release.PACKAGE_NAME}@{package_version}": release.tarball_integrity(
                    path
                )
                for path, package_version, _tag in release.release_entries(
                    VERSION, npm_dir
                )
            }
            published: set[str] = set()

            def npm_view(spec: str, field: str) -> str | None:
                if field == "name":
                    return release.PACKAGE_NAME
                return integrities[spec] if spec in published else None

            def npm_publish(command: list[str], *, check: bool) -> None:
                self.assertTrue(check)
                self.assertIn("--provenance", command)
                package_version = release.read_manifest(Path(command[2]))["version"]
                published.add(f"{release.PACKAGE_NAME}@{package_version}")

            with (
                patch.object(release, "npm_view", side_effect=npm_view),
                patch.object(release.subprocess, "run", side_effect=npm_publish) as run,
            ):
                release.publish(VERSION, npm_dir)

            self.assertEqual(run.call_count, 4)


if __name__ == "__main__":
    unittest.main()
